use crate::flashbots_mock::mount_flashbots_mock;
use crate::flashbots_mock::BundleRequest;
use crate::test_config::TestConfig;
use alloy_primitives::{address, TxHash, U256};
use alloy_provider::network::eip2718::Encodable2718;
use alloy_provider::network::TransactionResponse;
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, BlockNumberOrTag};
use alloy_rpc_types_eth::TransactionTrait;
use clap::Parser;
use eyre::{OptionExt, Result};
use kabu::broadcast::accounts::{AccountMonitorComponent, InitializeSignersOneShotBlockingComponent, SignersComponent};
use kabu::broadcast::broadcaster::{FlashbotsBroadcastComponent, RelayConfig};
use kabu::core::block_history::BlockHistoryComponent;
use kabu::core::blockchain::Blockchain;
use kabu::core::components::MevComponentChannels;
use kabu::core::router::SwapRouterComponent;
use kabu::defi::address_book::TokenAddressEth;
use kabu::defi::health_monitor::StuffingTxMonitorActor;
use kabu::defi::market::{fetch_and_add_pool_by_pool_id, fetch_state_and_add_pool};
use kabu::defi::pools::protocols::CurveProtocol;
use kabu::defi::pools::{CurvePool, PoolLoadersBuilder, PoolsLoadingConfig};
use kabu::defi::preloader::MarketStatePreloadedOneShotComponent;
use kabu::defi::price::PriceComponent;
use kabu::evm::db::KabuDBType;
use kabu::evm::utils::NWETH;
use kabu::execution::estimator::EvmEstimatorComponent;
use kabu::execution::multicaller::{MulticallerDeployer, MulticallerSwapEncoder};
use kabu::node::debug_provider::AnvilDebugProviderFactory;
use kabu::node::json_rpc::BlockProcessingComponent;
use kabu::strategy::backrun::{BackrunConfig, StateChangeArbComponent};
use kabu::strategy::merger::{ArbSwapPathMergerComponent, DiffPathMergerComponent, SamePathMergerComponent};
use kabu::types::blockchain::{debug_trace_block, ChainParameters, KabuDataTypesEthereum};
use kabu::types::entities::BlockHistory;
use kabu::types::events::{MarketEvents, MempoolEvents, SwapComposeMessage};
use kabu::types::market::{MarketState, PoolClass, PoolId, Token};
use kabu::types::swap::Swap;
use kabu_core_components::Component;
use kabu_node_config::NodeBlockComponentConfig;
use reth_tasks::TaskManager;
use std::env;
use std::fmt::{Display, Formatter};
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};
use wiremock::MockServer;

mod flashbots_mock;
mod test_config;

#[derive(Clone, Default, Debug)]
struct Stat {
    found_counter: usize,
    sign_counter: usize,
    best_profit_eth: U256,
    best_swap: Option<Swap>,
}

impl Display for Stat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.best_swap {
            Some(swap) => match swap.get_first_token() {
                Some(token) => {
                    write!(
                        f,
                        "Found: {} Ok: {} Profit : {} / ProfitEth : {} Path : {} ",
                        self.found_counter,
                        self.sign_counter,
                        token.to_float(swap.arb_profit()),
                        NWETH::to_float(swap.arb_profit_eth()),
                        swap
                    )
                }
                None => {
                    write!(
                        f,
                        "Found: {} Ok: {} Profit : {} / ProfitEth : {} Path : {} ",
                        self.found_counter,
                        self.sign_counter,
                        swap.arb_profit(),
                        swap.arb_profit_eth(),
                        swap
                    )
                }
            },
            _ => {
                write!(f, "NO BEST SWAP")
            }
        }
    }
}

#[allow(dead_code)]
fn parse_tx_hashes(tx_hash_vec: Vec<&str>) -> Result<Vec<TxHash>> {
    let mut ret: Vec<TxHash> = Vec::new();
    for tx_hash in tx_hash_vec {
        ret.push(tx_hash.parse()?);
    }
    Ok(ret)
}

#[derive(Parser, Debug)]
struct Commands {
    #[arg(short, long)]
    config: String,

    /// Timout in seconds after the test fails
    #[arg(short, long, default_value = "10")]
    timeout: u64,

    /// Wait xx seconds before start re-broadcasting
    #[arg(short, long, default_value = "1")]
    wait_init: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::from_filename(".env.test").ok();
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "debug,alloy_rpc_client=off,kabu_multicaller=trace".into());
    let fmt_layer = fmt::Layer::default().with_thread_ids(true).with_file(false).with_line_number(true).with_filter(env_filter);

    tracing_subscriber::registry().with(fmt_layer).init();

    let args = Commands::parse();
    let test_config = TestConfig::from_file(args.config.clone()).await?;
    let node_url = env::var("MAINNET_WS")?;
    let client = AnvilDebugProviderFactory::from_node_on_block(node_url, test_config.settings.block).await?;
    let priv_key = client.privkey()?.to_bytes().to_vec();

    let mut mock_server: Option<MockServer> = None;
    if test_config.modules.flashbots {
        // Start flashbots mock server
        mock_server = Some(MockServer::start().await);
        mount_flashbots_mock(mock_server.as_ref().unwrap()).await;
    }

    //let multicaller_address = MulticallerDeployer::new().deploy(client.clone(), priv_key.clone()).await?.address().ok_or_eyre("MULTICALLER_NOT_DEPLOYED")?;
    let multicaller_address = MulticallerDeployer::new()
        .set_code(client.clone(), address!("FCfCfcfC0AC30164AFdaB927F441F2401161F358"))
        .await?
        .address()
        .ok_or_eyre("MULTICALLER_NOT_DEPLOYED")?;
    info!("Multicaller deployed at {:?}", multicaller_address);

    let multicaller_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);

    let block_number = client.get_block_number().await?;
    info!("Current block_number={}", block_number);

    let block_header = client.get_block(block_number.into()).await?.unwrap().header;
    info!("Current block_header={:?}", block_header);

    let block_header_with_txes = client.get_block(block_number.into()).await?.unwrap();

    let state_db = KabuDBType::default();
    let market_state_instance = MarketState::new(state_db);

    // Add default tokens for price actor

    info!("Creating channels and task executor");
    // Create TaskExecutor using TaskManager for testing
    let task_manager = TaskManager::new(tokio::runtime::Handle::current());
    let task_executor = task_manager.executor();

    // Create Blockchain instance which manages all channels
    let blockchain = Blockchain::new(1); // Chain ID 1 for mainnet

    // Get channels from blockchain
    let new_block_headers_channel = blockchain.new_block_headers_channel();
    let new_block_with_tx_channel = blockchain.new_block_with_tx_channel();
    let new_block_state_update_channel = blockchain.new_block_state_update_channel();
    let new_block_logs_channel = blockchain.new_block_logs_channel();
    let market_events_channel = blockchain.market_events_channel();
    let mempool_events_channel = blockchain.mempool_events_channel();
    let pool_health_monitor_channel = blockchain.health_monitor_channel();

    // Get shared state from blockchain and create additional state
    let market_instance = blockchain.market();
    let mempool_instance = blockchain.mempool();
    let latest_block = blockchain.latest_block();

    // Update the market with our test tokens
    {
        let mut market_guard = market_instance.write().await;
        market_guard.add_token(Token::new_with_data(TokenAddressEth::USDC, Some("USDC".to_string()), None, Some(6), true, false));
        market_guard.add_token(Token::new_with_data(TokenAddressEth::USDT, Some("USDT".to_string()), None, Some(6), true, false));
        market_guard.add_token(Token::new_with_data(TokenAddressEth::WBTC, Some("WBTC".to_string()), None, Some(8), true, false));
        market_guard.add_token(Token::new_with_data(TokenAddressEth::DAI, Some("DAI".to_string()), None, Some(18), true, false));
    }

    // Create market state and other test-specific state
    let market_state = Arc::new(RwLock::new(market_state_instance));
    let block_history_state = Arc::new(RwLock::new(BlockHistory::new(10)));

    // Create MEV channels for swap compose and tx compose
    let mev_channels = MevComponentChannels::<KabuDBType>::default();

    // Update latest block with current block info
    latest_block.write().await.update(block_number, block_header.hash, None, None, None, None);

    let (_, post) = debug_trace_block(client.clone(), BlockId::Number(BlockNumberOrTag::Number(block_number)), true).await?;
    latest_block.write().await.update(
        block_number,
        block_header.hash,
        Some(block_header.clone()),
        Some(block_header_with_txes),
        None,
        Some(post),
    );

    info!("Starting initialize signers actor");

    let initialize_signers_actor = InitializeSignersOneShotBlockingComponent::new(Some(priv_key))
        .with_signers(mev_channels.signers.clone())
        .with_monitor(mev_channels.account_state.clone());
    match initialize_signers_actor.spawn(task_executor.clone()) {
        Err(e) => {
            error!("{}", e);
            panic!("Cannot initialize signers");
        }
        _ => info!("Signers have been initialized"),
    }
    // Give it a moment to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    for (token_name, token_config) in test_config.tokens {
        let symbol = token_config.symbol.unwrap_or(token_config.address.to_checksum(None));
        let name = token_config.name.unwrap_or(symbol.clone());
        let token = Token::new_with_data(
            token_config.address,
            Some(symbol),
            Some(name),
            Some(token_config.decimals.map_or(18, |x| x)),
            token_config.basic.unwrap_or_default(),
            token_config.middle.unwrap_or_default(),
        );
        if let Some(price_float) = token_config.price {
            let price_u256 = NWETH::from_float(price_float) * token.get_exp() / NWETH::get_exp();
            debug!("Setting price : {} -> {} ({})", token_name, price_u256, price_u256.to::<u128>());

            token.set_eth_price(Some(price_u256));
        };

        market_instance.write().await.add_token(token);
    }

    info!("Starting market state preload component");
    let market_state_preload_component = MarketStatePreloadedOneShotComponent::new(client.clone())
        .with_copied_account(multicaller_encoder.get_contract_address())
        .with_signers(mev_channels.signers.clone())
        .with_market_state(market_state.clone());
    match market_state_preload_component.spawn(task_executor.clone()) {
        Err(e) => {
            error!("{}", e)
        }
        _ => {
            info!("Market state preload component started successfully")
        }
    }

    info!("Starting node component");
    let node_block_component = BlockProcessingComponent::new(client.clone(), NodeBlockComponentConfig::all_enabled()).with_channels(
        Some(new_block_headers_channel.clone()),
        Some(new_block_with_tx_channel.clone()),
        Some(new_block_logs_channel.clone()),
        Some(new_block_state_update_channel.clone()),
    );
    match node_block_component.spawn(task_executor.clone()) {
        Err(e) => {
            error!("{}", e)
        }
        _ => {
            info!("Node component started successfully")
        }
    }

    info!("Starting account monitor component");
    let account_monitor_component = AccountMonitorComponent::new(
        client.clone(),
        mev_channels.account_state.clone(),
        mev_channels.signers.clone(),
        Duration::from_secs(1),
    );
    match account_monitor_component.spawn(task_executor.clone()) {
        Err(e) => {
            error!("{}", e);
            panic!("Cannot initialize account monitor");
        }
        _ => info!("Account monitor has been initialized"),
    }

    info!("Starting price component");
    let price_component = PriceComponent::new(client.clone()).only_once().with_market(market_instance.clone());
    match price_component.spawn(task_executor.clone()) {
        Err(e) => {
            error!("{}", e);
            panic!("Cannot initialize price component");
        }
        _ => info!("Price component has been initialized"),
    }
    // Give it a moment to complete since it runs only once
    tokio::time::sleep(Duration::from_millis(500)).await;

    let pool_loaders =
        Arc::new(PoolLoadersBuilder::<_, _, KabuDataTypesEthereum>::default_pool_loaders(client.clone(), PoolsLoadingConfig::default()));

    for (pool_name, pool_config) in test_config.pools {
        match pool_config.class {
            PoolClass::UniswapV2 | PoolClass::UniswapV3 => {
                debug!(address=%pool_config.address, class=%pool_config.class, "Loading pool");
                fetch_and_add_pool_by_pool_id(
                    client.clone(),
                    market_instance.clone(),
                    market_state.clone(),
                    pool_loaders.clone(),
                    PoolId::Address(pool_config.address),
                    pool_config.class,
                )
                .await?;

                debug!(address=%pool_config.address, class=%pool_config.class, "Loaded pool");
            }
            PoolClass::Curve => {
                debug!("Loading curve pool");
                if let Ok(curve_contract) = CurveProtocol::get_contract_from_code(client.clone(), pool_config.address).await {
                    let curve_pool = CurvePool::fetch_pool_data_with_default_encoder(client.clone(), curve_contract).await?;
                    fetch_state_and_add_pool::<_, _, _, KabuDataTypesEthereum>(
                        client.clone(),
                        market_instance.clone(),
                        market_state.clone(),
                        curve_pool.into(),
                    )
                    .await?;
                } else {
                    error!("CURVE_POOL_NOT_LOADED");
                }
                debug!("Loaded curve pool");
            }
            _ => {
                error!("Unknown pool class")
            }
        }
        let swap_path_len = market_instance.read().await.get_pool_paths(&PoolId::Address(pool_config.address)).unwrap_or_default().len();
        info!(
            "Loaded pool '{}' with address={}, pool_class={}, swap_paths={}",
            pool_name, pool_config.address, pool_config.class, swap_path_len
        );
    }

    info!("Starting block history component");
    let block_history_component = BlockHistoryComponent::new(client.clone()).with_channels(
        ChainParameters::ethereum(),
        latest_block.clone(),
        market_state.clone(),
        block_history_state.clone(),
        new_block_headers_channel.clone(),
        new_block_with_tx_channel.clone(),
        new_block_logs_channel.clone(),
        new_block_state_update_channel.clone(),
        market_events_channel.clone(),
    );
    match block_history_component.spawn(task_executor.clone()) {
        Err(e) => {
            error!("{}", e)
        }
        _ => {
            info!("Block history component started successfully")
        }
    }

    // Start estimator component
    let estimator_component = EvmEstimatorComponent::new_with_provider(multicaller_encoder.clone(), Some(client.clone()))
        .with_swap_compose_channel(mev_channels.swap_compose.clone());
    match estimator_component.spawn(task_executor.clone()) {
        Err(e) => error!("{e}"),
        _ => {
            info!("Estimator component started successfully")
        }
    }

    let health_monitor_component = StuffingTxMonitorActor::new(client.clone())
        .with_latest_block(latest_block.clone())
        .with_tx_compose_channel(mev_channels.tx_compose.clone())
        .with_market_events(market_events_channel.clone());
    match health_monitor_component.spawn(task_executor.clone()) {
        Ok(_) => {
            info!("Stuffing tx monitor component started")
        }
        Err(e) => {
            panic!("StuffingTxMonitorActor error {e}")
        }
    }

    // Start actor that encodes paths found
    if test_config.modules.encoder {
        info!("Starting swap router component");

        let swap_router_component =
            SwapRouterComponent::new(mev_channels.signers.clone(), mev_channels.account_state.clone(), mev_channels.swap_compose.clone());

        match swap_router_component.spawn(task_executor.clone()) {
            Err(e) => {
                error!("{}", e)
            }
            _ => {
                info!("Swap router component started successfully")
            }
        }
    }

    // Start signer actor that signs paths before broadcasting
    if test_config.modules.signer {
        info!("Starting signers component");
        let signers_component = SignersComponent::<_, _, KabuDBType, KabuDataTypesEthereum>::new(
            client.clone(),
            mev_channels.signers.clone(),
            mev_channels.account_state.clone(),
            120, // gas_price_buffer
        )
        .with_channels(mev_channels.swap_compose.clone(), mev_channels.swap_compose.clone());
        match signers_component.spawn(task_executor.clone()) {
            Err(e) => {
                error!("{}", e);
                panic!("Cannot start signers");
            }
            _ => info!("Signers component started"),
        }
    }

    // Start state change arb actor
    if test_config.modules.arb_block || test_config.modules.arb_mempool {
        info!("Starting state change arb component");
        let state_change_arb_component = StateChangeArbComponent::<_, _, KabuDBType, KabuDataTypesEthereum>::new(
            client.clone(),
            test_config.modules.arb_block,
            test_config.modules.arb_mempool,
            BackrunConfig::new_dumb(),
        )
        .with_market(market_instance.clone())
        .with_mempool(mempool_instance.clone())
        .with_latest_block(latest_block.clone())
        .with_market_state(market_state.clone())
        .with_block_history(block_history_state.clone())
        .with_mempool_events_channel(mempool_events_channel.clone())
        .with_market_events_channel(market_events_channel.clone())
        .with_swap_compose_channel(mev_channels.swap_compose.clone())
        .with_pool_health_monitor_channel(pool_health_monitor_channel.clone());
        match state_change_arb_component.spawn(task_executor.clone()) {
            Err(e) => {
                error!("{}", e)
            }
            _ => {
                info!("State change arb component started successfully")
            }
        }
    }

    // Swap path merger tries to build swap steps from swap lines
    if test_config.modules.arb_path_merger {
        info!("Starting swap path merger component");

        let swap_path_merger_component = ArbSwapPathMergerComponent::<KabuDBType>::new(multicaller_address)
            .with_latest_block(latest_block.clone())
            .with_market_events_channel(market_events_channel.clone())
            .with_compose_channel(mev_channels.swap_compose.clone());
        match swap_path_merger_component.spawn(task_executor.clone()) {
            Err(e) => {
                error!("{}", e)
            }
            _ => {
                info!("Swap path merger component started successfully")
            }
        }
    }

    // Same path merger tries to merge different stuffing tx to optimize swap line
    if test_config.modules.same_path_merger {
        let same_path_merger_component = SamePathMergerComponent::<_, _, KabuDBType>::new(client.clone())
            .with_market_state(market_state.clone())
            .with_latest_block(latest_block.clone())
            .with_market_events_channel(market_events_channel.clone())
            .with_compose_channel(mev_channels.swap_compose.clone());
        match same_path_merger_component.spawn(task_executor.clone()) {
            Err(e) => {
                error!("{}", e)
            }
            _ => {
                info!("Same path merger component started successfully")
            }
        }
    }
    if test_config.modules.flashbots {
        let relays = vec![RelayConfig { id: 1, url: mock_server.as_ref().unwrap().uri(), name: "relay".to_string(), no_sign: Some(false) }];
        let flashbots_broadcast_component =
            FlashbotsBroadcastComponent::new(None, true)?.with_relays(relays)?.with_channel(mev_channels.tx_compose.clone());
        match flashbots_broadcast_component.spawn(task_executor.clone()) {
            Err(e) => {
                error!("{}", e)
            }
            _ => {
                info!("Flashbots broadcast component started successfully")
            }
        }
    }

    // Diff path merger tries to merge all found swaplines into one transaction
    let diff_path_merger_component = DiffPathMergerComponent::<KabuDBType>::new()
        .with_market_events_channel(market_events_channel.clone())
        .with_compose_channel(mev_channels.swap_compose.clone());
    match diff_path_merger_component.spawn(task_executor.clone()) {
        Err(e) => {
            error!("{}", e)
        }
        _ => {
            info!("Diff path merger component started successfully")
        }
    }

    // #### Blockchain events
    // we need to wait for all components to start. For the CI it can be a bit longer
    tokio::time::sleep(Duration::from_secs(args.wait_init)).await;

    let next_block_base_fee = ChainParameters::ethereum().calc_next_block_base_fee(
        block_header.gas_used,
        block_header.gas_limit,
        block_header.base_fee_per_gas.unwrap_or_default(),
    );

    // Sending block header update message
    if let Err(e) = market_events_channel.send(MarketEvents::BlockHeaderUpdate {
        block_number: block_header.number,
        block_hash: block_header.hash,
        timestamp: block_header.timestamp,
        base_fee: block_header.base_fee_per_gas.unwrap_or_default(),
        next_base_fee: next_block_base_fee,
    }) {
        error!("{}", e);
    }

    // Sending block state update message
    if let Err(e) = market_events_channel.send(MarketEvents::BlockStateUpdate { block_hash: block_header.hash }) {
        error!("{}", e);
    }

    // #### RE-BROADCASTER
    //starting broadcasting transactions from eth to anvil
    let client_clone = client.clone();
    tokio::spawn(async move {
        info!("Re-broadcaster task started");

        for (_, tx_config) in test_config.txs.iter() {
            debug!("Fetching original tx {}", tx_config.hash);
            let Some(tx) = client_clone.get_transaction_by_hash(tx_config.hash).await.unwrap() else {
                panic!("Cannot get tx: {}", tx_config.hash);
            };

            let from = tx.from();
            let to = tx.to().unwrap_or_default();

            match tx_config.send.to_lowercase().as_str() {
                "mempool" => {
                    let mut mempool_guard = mempool_instance.write().await;
                    let tx_hash: TxHash = tx.tx_hash();

                    mempool_guard.add_tx(tx.clone());
                    if let Err(e) = mempool_events_channel.send(MempoolEvents::MempoolActualTxUpdate { tx_hash }) {
                        error!("{e}");
                    }
                }
                "block" => match client_clone.send_raw_transaction(tx.inner.encoded_2718().as_slice()).await {
                    Ok(p) => {
                        debug!("Transaction sent {}", p.tx_hash());
                    }
                    Err(e) => {
                        error!("Error sending transaction : {e}");
                    }
                },
                _ => {
                    debug!("Incorrect action {} for : hash {} from {} to {}  ", tx_config.send, tx.tx_hash(), from, to);
                }
            }
        }
    });

    println!("Test '{}' is started!", args.config);

    let mut tx_compose_sub = mev_channels.swap_compose.subscribe();

    let mut stat = Stat::default();
    let timeout_duration = Duration::from_secs(args.timeout);

    loop {
        tokio::select! {
            msg = tx_compose_sub.recv() => {
                match msg {
                    Ok(msg) => match msg.inner {
                        SwapComposeMessage::Ready(ready_message) => {
                            debug!(swap=%ready_message.swap, "Ready message");
                            stat.sign_counter += 1;

                            if stat.best_profit_eth < ready_message.swap.arb_profit_eth() {
                                stat.best_profit_eth = ready_message.swap.arb_profit_eth();
                                stat.best_swap = Some(ready_message.swap.clone());
                            }

                            if let Some(swaps_ok) = test_config.assertions.swaps_ok {
                                if stat.sign_counter >= swaps_ok  {
                                    break;
                                }
                            }
                        }
                        SwapComposeMessage::Prepare(encode_message) => {
                            debug!(swap=%encode_message.swap, "Prepare message");
                            stat.found_counter += 1;
                        }
                        _ => {}
                    },
                    Err(error) => {
                        error!(%error, "tx_compose_sub.recv")
                    }
                }
            }
            msg = tokio::time::sleep(timeout_duration) => {
                debug!(?msg, "Timed out");
                break;
            }
        }
    }
    if test_config.modules.flashbots {
        // wait for flashbots mock server to receive all requests
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Some(last_requests) = mock_server.unwrap().received_requests().await {
            if last_requests.is_empty() {
                println!("Mock server did not received any request!")
            } else {
                println!("Received {} flashbots requests", last_requests.len());
                for request in last_requests {
                    let bundle_request: BundleRequest = serde_json::from_slice(&request.body)?;
                    println!(
                        "bundle_count={}, target_blocks={:?}, txs_in_bundles={:?}",
                        bundle_request.params.len(),
                        bundle_request.params.iter().map(|b| b.target_block).collect::<Vec<_>>(),
                        bundle_request.params.iter().map(|b| b.transactions.len()).collect::<Vec<_>>()
                    );
                    // print all transactions
                    for bundle in bundle_request.params {
                        println!("Bundle with {} transactions", bundle.transactions.len());
                    }
                }
            }
        } else {
            println!("Mock server did not received any request!")
        }
    }

    println!("\n\n-------------------\nStat : {stat}\n-------------------\n");

    if let Some(swaps_encoded) = test_config.assertions.swaps_encoded {
        if swaps_encoded > stat.found_counter {
            println!("Test failed. Not enough encoded swaps : {} need {}", stat.found_counter, swaps_encoded);
            exit(1)
        } else {
            println!("Test passed. Encoded swaps : {} required {}", stat.found_counter, swaps_encoded);
        }
    }
    if let Some(swaps_ok) = test_config.assertions.swaps_ok {
        if swaps_ok > stat.sign_counter {
            println!("Test failed. Not enough verified swaps : {} need {}", stat.sign_counter, swaps_ok);
            exit(1)
        } else {
            println!("Test passed. swaps : {} required {}", stat.sign_counter, swaps_ok);
        }
    }
    if let Some(best_profit) = test_config.assertions.best_profit_eth {
        if NWETH::from_float(best_profit) > stat.best_profit_eth {
            println!("Profit is too small {} need {}", NWETH::to_float(stat.best_profit_eth), best_profit);
            exit(1)
        } else {
            println!("Test passed. best profit : {} > {}", NWETH::to_float(stat.best_profit_eth), best_profit);
        }
    }

    Ok(())
}
