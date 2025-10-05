use crate::flashbots_mock::BundleRequest;
use crate::flashbots_mock::mount_flashbots_mock;
use crate::test_config::TestConfig;
use alloy_network::Ethereum;
use alloy_primitives::{Address, TxHash, U256, address};
use alloy_provider::Provider;
use alloy_provider::network::eip2718::Encodable2718;
use alloy_rpc_types::{BlockId, BlockNumberOrTag};
use alloy_rpc_types_eth::TransactionTrait;
use chrono::Local;
use clap::Parser;
use eyre::{OptionExt, Result, eyre};
use kabu::core::blockchain::Blockchain;
use kabu::core::blockchain::BlockchainState;
use kabu::core::config::KabuConfig;
use kabu::defi::address_book::TokenAddressEth;
use kabu::defi::pools::PoolsLoadingConfig;
use kabu::defi::pools::{UniswapV2Pool, UniswapV3Pool};
use kabu::evm::db::{AlloyDB, KabuDB};
use kabu::evm::utils::NWETH;
use kabu::execution::multicaller::{MulticallerDeployer, MulticallerSwapEncoder};
use kabu::node::debug_provider::AnvilDebugProviderFactory;
use kabu::node::reth_api::KabuRethProviderWrapper;
use kabu::strategy::backrun::BackrunConfig;
use kabu::types::blockchain::{ChainParameters, debug_trace_block};
use kabu::types::entities::LoomTxSigner;
use kabu::types::events::{MarketEvents, MempoolEvents, SwapComposeMessage};
use kabu::types::market::{MarketState, PoolClass, Token};
use kabu::types::market::{Pool, PoolWrapper, RequiredStateReader};
use kabu::types::swap::Swap;
use kabu_core_components::MevComponentChannels;
use kabu_core_node::{KabuContext, KabuEthereumNode, NodeBuilder, NodeConfig};
use reth::chainspec::MAINNET;
use reth_node_ethereum::{EthEvmConfig, EthereumNode};
use reth_storage_rpc_provider::{RpcBlockchainProvider, RpcBlockchainProviderConfig};
use reth_tasks::TaskManager;
use std::env;
use std::fmt::{Display, Formatter};
use std::process::exit;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};
use wiremock::MockServer;

mod flashbots_mock;
mod test_config;

use reth::primitives::{EthPrimitives, TxTy};
use reth::rpc::compat::{TryFromBlockResponse, TryFromTransactionResponse};
use reth_ethereum_primitives::Receipt;
use reth_primitives::Block;
use reth_primitives_traits::block::RecoveredBlock;
use reth_primitives_traits::{Block as BlockTrait, BlockTy, SignerRecoverable};
use revm::database::BundleState;
use std::io::{self, Write};

#[derive(Clone, Default, Debug)]
struct Stat {
    found_counter: usize,
    sign_counter: usize,
    best_profit_eth: U256,
    best_swap: Option<Swap>,
    start_time: Option<Instant>,
}

impl Display for Stat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Runtime
        if let Some(start) = self.start_time {
            let elapsed = start.elapsed();
            writeln!(f, "  Runtime: {:.3}s", elapsed.as_secs_f64())?;
        }

        // Summary
        writeln!(f, "  Opportunities Found: {}", self.found_counter)?;
        writeln!(
            f,
            "  Opportunities Verified: {} ({:.1}%)",
            self.sign_counter,
            if self.found_counter > 0 { (self.sign_counter as f64 / self.found_counter as f64) * 100.0 } else { 0.0 }
        )?;

        // Best arbitrage
        match &self.best_swap {
            Some(swap) => {
                writeln!(f, "\n  Best Arbitrage:")?;
                match swap.get_first_token() {
                    Some(token) => {
                        writeln!(f, "    Token Profit: {} {}", token.to_float(swap.arb_profit()), token.get_symbol())?;
                    }
                    None => {
                        writeln!(f, "    Raw Profit: {}", swap.arb_profit())?;
                    }
                }
                writeln!(f, "    ETH Profit: {} ETH", NWETH::to_float(swap.arb_profit_eth()))?;
                writeln!(f, "    Path: {swap}")?;
            }
            None => {
                writeln!(f, "\n  No profitable arbitrage found")?;
            }
        }

        Ok(())
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

use std::ffi::OsStr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
struct Commands {
    /// Timeout in seconds after the test fails
    #[arg(short, long, default_value = "10")]
    timeout: u64,

    /// Wait xx seconds before start re-broadcasting
    #[arg(short, long, default_value = "1")]
    wait_init: u64,

    /// Flashbots collection wait time in seconds
    #[arg(short = 'f', long, default_value = "0")]
    flashbots_wait: u64,

    /// Path to test file or directory containing tests
    path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::from_filename(".env.test").ok();
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "debug,alloy_rpc_client=off,kabu_multicaller=trace".into());
    let fmt_layer = fmt::Layer::default().with_thread_ids(true).with_file(false).with_line_number(true).with_filter(env_filter);

    tracing_subscriber::registry().with(fmt_layer).init();

    let args = Commands::parse();

    if args.path.is_file() {
        // Run single test
        println!("[{}] Running single test: {}", Local::now().format("%H:%M:%S.%3f"), args.path.display());
        execute_test(args.path.clone(), &args).await?;
    } else if args.path.is_dir() {
        // Find and run multiple tests
        let mut test_files = Vec::new();
        for entry in std::fs::read_dir(&args.path)? {
            let path = entry?.path();
            if path.extension() == Some(OsStr::new("toml"))
                && path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("test_")).unwrap_or(false)
            {
                test_files.push(path);
            }
        }
        test_files.sort();

        println!("[{}] Found {} tests in {}", Local::now().format("%H:%M:%S.%3f"), test_files.len(), args.path.display());

        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for test_file in test_files {
            // Quick check if test is disabled
            let test_config = TestConfig::from_file(test_file.to_string_lossy().to_string()).await?;

            if test_config.settings.disabled {
                skipped += 1;
                println!("\n[{}] SKIPPED: {} (disabled)", Local::now().format("%H:%M:%S.%3f"), test_file.display());
                continue;
            }

            println!("\n{}", "=".repeat(70));
            println!("[{}] Running: {}", Local::now().format("%H:%M:%S.%3f"), test_file.display());
            println!("{}", "=".repeat(70));

            match execute_test(test_file, &args).await {
                Ok(()) => {
                    passed += 1;
                    println!("[{}] ✓ Test passed", Local::now().format("%H:%M:%S.%3f"));
                }
                Err(e) => {
                    failed += 1;
                    eprintln!("[{}] ✗ Test failed: {}", Local::now().format("%H:%M:%S.%3f"), e);
                }
            }
        }

        println!("\n{}", "=".repeat(70));
        println!("[{}] TEST SUMMARY: {} passed, {} failed, {} skipped", Local::now().format("%H:%M:%S.%3f"), passed, failed, skipped);
        println!("{}", "=".repeat(70));

        if failed > 0 {
            std::process::exit(1);
        }
    } else {
        return Err(eyre!("Path does not exist: {}", args.path.display()));
    }

    Ok(())
}

// Complete test execution - everything happens here for isolation
async fn execute_test(test_path: PathBuf, args: &Commands) -> Result<()> {
    let test_config = TestConfig::from_file(test_path.to_string_lossy().to_string()).await?;

    // Check if test is disabled
    if test_config.settings.disabled {
        println!("[{}] SKIPPED: Test disabled - {}", Local::now().format("%H:%M:%S.%3f"), test_path.display());
        return Ok(());
    }

    println!("[{}] TEST STARTED: {}", Local::now().format("%H:%M:%S.%3f"), test_path.display());
    println!("[{}] Block: {} | Timeout: {}s", Local::now().format("%H:%M:%S.%3f"), test_config.settings.block, args.timeout);

    // Create a fresh Anvil instance for this test
    let node_url = env::var("MAINNET_WS")?;
    let client = AnvilDebugProviderFactory::from_node_on_block(node_url, test_config.settings.block).await?;
    let priv_key = client.privkey()?.to_bytes().to_vec();

    let mut mock_server: Option<MockServer> = None;
    if test_config.modules.flashbots {
        // Start flashbots mock server
        mock_server = Some(MockServer::start().await);
        mount_flashbots_mock(mock_server.as_ref().unwrap()).await;
    }

    let multicaller_address = MulticallerDeployer::new()
        .set_code(client.clone(), address!("FCfCfcfC0AC30164AFdaB927F441F2401161F358"))
        .await?
        .address()
        .ok_or_eyre("MULTICALLER_NOT_DEPLOYED")?;
    info!("Multicaller deployed at {:?}", multicaller_address);

    let multicaller_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);

    let block_number = client.get_block_number().await?;
    info!("Current block_number={}", block_number);

    let block_response = client.get_block(block_number.into()).await?.unwrap();
    let block = <BlockTy<EthPrimitives> as TryFromBlockResponse<Ethereum>>::from_block_response(block_response)?;
    let block_header = block.header().clone();
    info!("Current block_header={:?}", block_header);

    let block_response = client.get_block(block_number.into()).await?.unwrap();

    let block_header_with_txes = <BlockTy<EthPrimitives> as TryFromBlockResponse<Ethereum>>::from_block_response(block_response)?;

    // Create AlloyDB connected to the forked Anvil instance
    let alloy_db =
        AlloyDB::new(client.clone(), BlockId::Number(BlockNumberOrTag::Number(block_number))).ok_or_eyre("Failed to create AlloyDB")?;
    let state_db = KabuDB::new().with_ext_db(alloy_db);
    let market_state_instance = MarketState::new(state_db);

    // Add default tokens for price actor

    info!("Creating blockchain and initial state");
    // Create Blockchain instance which manages all channels
    // Disable influxdb for test runner
    let blockchain = Blockchain::new_with_config(1, false); // Chain ID 1 for mainnet, influxdb disabled

    // Create blockchain state
    let blockchain_state = BlockchainState::<KabuDB, EthPrimitives>::new_with_market_state(market_state_instance);

    // Get references we need for setup
    let market_instance = blockchain.market();

    // Update the market with our test tokens
    {
        let mut market_guard = market_instance.write().await;
        market_guard.add_token(Token::new_with_data(TokenAddressEth::USDC, Some("USDC".to_string()), None, Some(6), true, false));
        market_guard.add_token(Token::new_with_data(TokenAddressEth::USDT, Some("USDT".to_string()), None, Some(6), true, false));
        market_guard.add_token(Token::new_with_data(TokenAddressEth::WBTC, Some("WBTC".to_string()), None, Some(8), true, false));
        market_guard.add_token(Token::new_with_data(TokenAddressEth::DAI, Some("DAI".to_string()), None, Some(18), true, false));
    }

    // Update latest block with current block info
    let (_, _post) = debug_trace_block(client.clone(), BlockId::Number(BlockNumberOrTag::Number(block_number)), true).await?;

    info!("Starting initialize signers actor");

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

    // Create config for test environment
    let config = KabuConfig {
        remote_node: None, // No remote node needed for test environment
        signers: Vec::new(),
        builders: Vec::new(),
        multicaller_address,
        influxdb: None,
        webserver: None,
        database: None, // Database is optional for test runner
    };

    // Create BackrunConfig from test settings
    let backrun_config = BackrunConfig::new_dumb();

    // Configure which pools to load based on test config
    let mut pools_config = PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default());
    for pool_config in test_config.pools.values() {
        match pool_config.class {
            PoolClass::UniswapV2 => pools_config = pools_config.enable(PoolClass::UniswapV2),
            PoolClass::UniswapV3 => pools_config = pools_config.enable(PoolClass::UniswapV3),
            PoolClass::Curve => pools_config = pools_config.enable(PoolClass::Curve),
            _ => {}
        }
    }

    println!("[{}] Building Kabu MEV components...", Local::now().format("%H:%M:%S.%3f"));
    info!("Building Kabu MEV components with KabuEthereumNode");

    // For test runner, we don't need a database pool
    info!("Test runner using no database pool");

    // Create channels first so we can use them throughout the test
    let mev_channels = MevComponentChannels::<KabuDB>::default();

    // Create RpcBlockchainProvider for test environment and wrap it with canonical state support
    let rpc_config = RpcBlockchainProviderConfig { compute_state_root: false, reth_rpc_support: false };
    let inner_provider =
        RpcBlockchainProvider::<_, EthereumNode, Ethereum>::new_with_config(client.clone(), rpc_config).with_chain_spec(MAINNET.clone());
    let reth_provider = KabuRethProviderWrapper::new(inner_provider);

    // Create EVM config
    let evm_config = EthEvmConfig::mainnet();

    // Create node configuration
    let node_config = NodeConfig::new()
        .chain_id(1)
        .config(config.clone())
        .backrun_config(backrun_config.clone())
        .multicaller_address(multicaller_address)
        .swap_encoder(multicaller_encoder.clone())
        .pools_config(pools_config.clone())
        .enable_web_server(false); // Disable web server for test runner

    // Create context with all providers and custom channels for testing
    let mut context =
        KabuContext::new(reth_provider.clone(), client.clone(), evm_config, blockchain.clone(), blockchain_state.clone(), node_config);

    // Override channels with our test channels
    context.channels = mev_channels.clone();

    // Create TaskExecutor
    let task_manager = TaskManager::new(tokio::runtime::Handle::current());
    let task_executor = task_manager.executor();

    // Initialize test signer BEFORE launching components
    println!("[{}] Initializing test signer...", Local::now().format("%H:%M:%S.%3f"));
    {
        let signers = mev_channels.signers.clone();
        let account_state = mev_channels.account_state.clone();

        // Add the private key directly
        let signer = signers.write().await.add_privkey(alloy_primitives::Bytes::from(priv_key));

        // Get the actual nonce and balance from Anvil
        let nonce = client.get_transaction_count(signer.address()).await?;
        let balance = client.get_balance(signer.address()).await?;

        // Set the correct account state
        account_state.write().await.add_account(signer.address()).set_nonce(nonce).set_balance(Address::default(), balance); // ETH balance

        println!(
            "[{}] Test signer initialized: {:?} (nonce={}, balance={})",
            Local::now().format("%H:%M:%S.%3f"),
            signer.address(),
            nonce,
            balance
        );
    }

    // Build and launch components - now SignersComponent will have signers configured
    let _handle = NodeBuilder::new()
        .with_context(context)
        .with_components(KabuEthereumNode::components().build())
        .launch(task_executor.clone())
        .await?;

    // Give components time to initialize
    println!("[{}] Waiting for component initialization...", Local::now().format("%H:%M:%S.%3f"));
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Manually load test pools since automatic loader may not work in test environment
    let total_pools = test_config.pools.len();
    if total_pools > 0 {
        println!("[{}] Loading {} test pools manually...", Local::now().format("%H:%M:%S.%3f"), total_pools);

        let mut pools_loaded = 0;

        // Get access to the market state to update the state DB
        let market_state_db = blockchain_state.market_state_commit();

        for (pool_name, pool_config) in &test_config.pools {
            match pool_config.class {
                PoolClass::UniswapV2 => {
                    // For UniswapV2, we need to fetch pool data from chain
                    match UniswapV2Pool::fetch_pool_data(client.clone(), pool_config.address).await {
                        Ok(pool) => {
                            // Load pool state into state DB
                            match pool.get_state_required() {
                                Ok(state_required) => {
                                    match RequiredStateReader::fetch_calls_and_slots(client.clone(), state_required, Some(block_number))
                                        .await
                                    {
                                        Ok(state_update) => {
                                            market_state_db.write().await.state_db.apply_geth_update(state_update);
                                            debug!("Applied state update for UniswapV2 pool {}", pool_name);
                                        }
                                        Err(e) => {
                                            error!("Failed to fetch state for UniswapV2 pool {}: {}", pool_name, e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to get required state for UniswapV2 pool {}: {}", pool_name, e);
                                }
                            }

                            if let Err(e) = market_instance.write().await.add_pool(PoolWrapper::from(pool)) {
                                error!("Failed to add UniswapV2 pool to market: {}", e);
                            } else {
                                pools_loaded += 1;
                                println!("  ✓ Loaded {} (UniswapV2) at {}", pool_name, pool_config.address);
                            }
                        }
                        Err(e) => {
                            error!("Failed to load UniswapV2 pool {}: {}", pool_name, e);
                        }
                    }
                }
                PoolClass::UniswapV3 => {
                    // For UniswapV3, we need to fetch pool data from chain
                    match UniswapV3Pool::fetch_pool_data(client.clone(), pool_config.address).await {
                        Ok(pool) => {
                            // Load pool state into state DB
                            match pool.get_state_required() {
                                Ok(state_required) => {
                                    match RequiredStateReader::fetch_calls_and_slots(client.clone(), state_required, Some(block_number))
                                        .await
                                    {
                                        Ok(state_update) => {
                                            market_state_db.write().await.state_db.apply_geth_update(state_update);
                                            debug!("Applied state update for UniswapV3 pool {}", pool_name);
                                        }
                                        Err(e) => {
                                            error!("Failed to fetch state for UniswapV3 pool {}: {}", pool_name, e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to get required state for UniswapV3 pool {}: {}", pool_name, e);
                                }
                            }

                            if let Err(e) = market_instance.write().await.add_pool(PoolWrapper::from(pool)) {
                                error!("Failed to add UniswapV3 pool to market: {}", e);
                            } else {
                                pools_loaded += 1;
                                println!("  ✓ Loaded {} (UniswapV3) at {}", pool_name, pool_config.address);
                            }
                        }
                        Err(e) => {
                            error!("Failed to load UniswapV3 pool {}: {}", pool_name, e);
                        }
                    }
                }
                _ => {
                    warn!("Unsupported pool class {:?} for {}", pool_config.class, pool_name);
                }
            }
        }

        println!("[{}] Loaded {}/{} test pools", Local::now().format("%H:%M:%S.%3f"), pools_loaded, total_pools);
    }

    // Get references for event sending
    // Use channels from MevComponentChannels (these are what components are listening to)
    let market_events_channel = mev_channels.market_events.clone();
    let mempool_events_channel = mev_channels.mempool_events.clone();
    // But mempool instance comes from blockchain
    let mempool_instance = blockchain.mempool();

    // #### Blockchain events
    // we need to wait for all components to start. For the CI it can be a bit longer
    let components_wait = args.wait_init;
    println!("[{}] Waiting {}s for all components to initialize...", Local::now().format("%H:%M:%S.%3f"), components_wait);

    // Show countdown
    for i in (1..=components_wait).rev() {
        print!("\r[{}] Starting in {}s... ", Local::now().format("%H:%M:%S.%3f"), i);
        io::stdout().flush().unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!("\r[{}] Components ready!              ", Local::now().format("%H:%M:%S.%3f"));

    let next_block_base_fee = ChainParameters::ethereum().calc_next_block_base_fee(
        block_header.gas_used,
        block_header.gas_limit,
        block_header.base_fee_per_gas.unwrap_or_default(),
    );

    // Send canonical state notification for the block
    // Convert the block and state to the format needed for canonical notification
    // Create an empty BundleState for testing
    let bundle_state = BundleState::default();

    let execution_outcome = reth_execution_types::ExecutionOutcome::<Receipt> {
        bundle: bundle_state,
        receipts: vec![vec![]], // Empty receipts for now
        first_block: block_header.number,
        requests: vec![], // requests
    };

    // We need to recover senders for the block
    // For backtest, we can use empty senders since we're not validating signatures
    // Convert alloy Block to reth Block
    let reth_block = Block {
        header: block_header.clone(),
        body: alloy_consensus::BlockBody {
            transactions: block_header_with_txes.body().transactions.clone(),
            ommers: vec![],
            withdrawals: block_header_with_txes.body().withdrawals.clone(),
        },
    };

    // Create RecoveredBlock with empty senders for testing
    let senders = vec![Address::ZERO; reth_block.body.transactions.len()];
    let recovered_block = RecoveredBlock::new_unhashed(reth_block, senders);

    if let Err(e) = reth_provider.send_canonical_commit(recovered_block, execution_outcome) {
        error!("Failed to send canonical state notification: {}", e);
    } else {
        info!(
            "Sent canonical state notification for block {} with {} transactions",
            block_header.number,
            block_header_with_txes.body().transactions.len()
        );
    }

    // Sending block header update message for market events
    if let Err(e) = market_events_channel.send(MarketEvents::BlockHeaderUpdate {
        block_number: block_header.number,
        block_hash: block_header.hash_slow(),
        timestamp: block_header.timestamp,
        base_fee: block_header.base_fee_per_gas.unwrap_or_default(),
        next_base_fee: next_block_base_fee,
    }) {
        error!("Failed to send BlockHeaderUpdate: {}", e);
    } else {
        info!("Sent BlockHeaderUpdate for block {}", block_header.number);
    }

    // #### RE-BROADCASTER
    //starting broadcasting transactions from eth to anvil
    let client_clone = client.clone();
    let mempool_events_channel_clone = mempool_events_channel.clone();
    let mempool_instance_clone = mempool_instance.clone();
    let market_events_channel_clone = market_events_channel.clone();
    let block_hash = block_header.hash_slow();
    tokio::spawn(async move {
        info!("Re-broadcaster task started");

        let total_txs = test_config.txs.len();
        if total_txs > 0 {
            info!("Re-broadcasting {} transactions", total_txs);
        }

        // Add delay to ensure components are ready
        tokio::time::sleep(Duration::from_millis(100)).await;

        for (_, tx_config) in test_config.txs.iter() {
            debug!("Fetching original tx {}", tx_config.hash);
            let Some(transaction_response) = client_clone.get_transaction_by_hash(tx_config.hash).await.unwrap() else {
                panic!("Cannot get tx: {}", tx_config.hash);
            };

            let tx =
                <TxTy<EthPrimitives> as TryFromTransactionResponse<Ethereum>>::from_transaction_response(transaction_response).unwrap();

            let from = tx.recover_signer().unwrap();
            let to = tx.to().unwrap_or_default();

            match tx_config.send.to_lowercase().as_str() {
                "mempool" => {
                    let mut mempool_guard = mempool_instance_clone.write().await;
                    let tx_hash: TxHash = *tx.tx_hash();

                    mempool_guard.add_tx(tx.clone());
                    drop(mempool_guard); // Release lock before sending event

                    if let Err(e) = mempool_events_channel_clone.send(MempoolEvents::MempoolActualTxUpdate { tx_hash }) {
                        error!("Failed to send mempool event: {}", e);
                    } else {
                        info!("Sent mempool event for tx {}", tx_hash);
                    }
                }
                "block" => match client_clone.send_raw_transaction(tx.encoded_2718().as_slice()).await {
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

        // Send block state update after all transactions are added to mempool
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Err(e) = market_events_channel_clone.send(MarketEvents::BlockStateUpdate { block_hash }) {
            error!("Failed to send block state update: {}", e);
        } else {
            info!("Sent block state update for block {}", block_hash);
        }
    });

    let test_start_time = Instant::now();
    println!("[{}] Waiting for arbitrage opportunities...", Local::now().format("%H:%M:%S.%3f"));

    let mut tx_compose_sub = mev_channels.swap_compose.subscribe();

    let mut stat = Stat { start_time: Some(test_start_time), ..Default::default() };
    let timeout_duration = Duration::from_secs(args.timeout);
    let mut last_update = Instant::now();
    let update_interval = Duration::from_secs(5);

    loop {
        tokio::select! {
            msg = tx_compose_sub.recv() => {
                match msg {
                    Ok(msg) => match msg.inner {
                        SwapComposeMessage::Ready(ready_message) => {
                            debug!(swap=%ready_message.swap, "Ready message");
                            stat.sign_counter += 1;
                            println!(
                                "[{}] Verified swap #{} - Profit: {} ETH",
                                Local::now().format("%H:%M:%S.%3f"),
                                stat.sign_counter,
                                NWETH::to_float(ready_message.swap.arb_profit_eth())
                            );

                            if stat.best_profit_eth < ready_message.swap.arb_profit_eth() {
                                stat.best_profit_eth = ready_message.swap.arb_profit_eth();
                                stat.best_swap = Some(ready_message.swap.clone());
                            }

                            if let Some(swaps_ok) = test_config.assertions.swaps_ok
                                && stat.sign_counter >= swaps_ok  {
                                    break;
                                }
                        }
                        SwapComposeMessage::Prepare(encode_message) => {
                            debug!(swap=%encode_message.swap, "Prepare message");
                            stat.found_counter += 1;
                            if stat.found_counter == 1 {
                                println!("[{}] Found first arbitrage opportunity!", Local::now().format("%H:%M:%S.%3f"));
                            }
                        }
                        _ => {}
                    },
                    Err(error) => {
                        error!(%error, "tx_compose_sub.recv")
                    }
                }
            }
            _ = tokio::time::sleep(update_interval) => {
                if last_update.elapsed() >= update_interval {
                    let elapsed = test_start_time.elapsed().as_secs();
                    let remaining = timeout_duration.as_secs().saturating_sub(elapsed);
                    println!(
                        "[{}] Status: {} found, {} verified | Elapsed: {}s, Remaining: {}s",
                        Local::now().format("%H:%M:%S.%3f"),
                        stat.found_counter,
                        stat.sign_counter,
                        elapsed,
                        remaining
                    );
                    last_update = Instant::now();

                    if elapsed >= timeout_duration.as_secs() {
                        println!("[{}] Test timeout reached ({}s)", Local::now().format("%H:%M:%S.%3f"), timeout_duration.as_secs());
                        break;
                    }
                }
            }
        }
    }
    if test_config.modules.flashbots && args.flashbots_wait > 0 {
        // wait for flashbots mock server to receive all requests
        println!(
            "[{}] Waiting {}s for Flashbots mock server to collect requests...",
            Local::now().format("%H:%M:%S.%3f"),
            args.flashbots_wait
        );
        for i in (1..=args.flashbots_wait).rev() {
            print!("\r[{}] Flashbots collection: {}s remaining... ", Local::now().format("%H:%M:%S.%3f"), i);
            io::stdout().flush().unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        println!("\r[{}] Flashbots collection complete!                    ", Local::now().format("%H:%M:%S.%3f"));
        if let Some(last_requests) = mock_server.unwrap().received_requests().await {
            if last_requests.is_empty() {
                println!("[{}] Flashbots: No requests received", Local::now().format("%H:%M:%S.%3f"))
            } else {
                println!("[{}] Flashbots: {} requests received", Local::now().format("%H:%M:%S.%3f"), last_requests.len());
                for request in last_requests {
                    let bundle_request: BundleRequest = serde_json::from_slice(&request.body)?;
                    println!(
                        "[{}]   Bundles: {} | Blocks: {:?} | Txs: {:?}",
                        Local::now().format("%H:%M:%S.%3f"),
                        bundle_request.params.len(),
                        bundle_request.params.iter().map(|b| b.target_block).collect::<Vec<_>>(),
                        bundle_request.params.iter().map(|b| b.transactions.len()).collect::<Vec<_>>()
                    );
                    // print all transactions
                    for bundle in bundle_request.params {
                        println!("[{}]     Bundle: {} tx(s)", Local::now().format("%H:%M:%S.%3f"), bundle.transactions.len());
                    }
                }
            }
        } else {
            println!("[{}] Flashbots: Recording disabled", Local::now().format("%H:%M:%S.%3f"))
        }
    }

    println!("\n[{}] TEST RESULTS\n{}", Local::now().format("%H:%M:%S.%3f"), "=".repeat(50));
    println!("{stat}");
    println!("{}", "=".repeat(50));

    if let Some(swaps_encoded) = test_config.assertions.swaps_encoded {
        if swaps_encoded > stat.found_counter {
            println!(
                "\n[{}] FAILED: Encoded swaps\n  Expected: >= {}\n  Actual: {}",
                Local::now().format("%H:%M:%S.%3f"),
                swaps_encoded,
                stat.found_counter
            );
            exit(1)
        } else {
            println!("[{}] PASSED: Encoded swaps ({})", Local::now().format("%H:%M:%S.%3f"), stat.found_counter);
        }
    }
    if let Some(swaps_ok) = test_config.assertions.swaps_ok {
        if swaps_ok > stat.sign_counter {
            println!(
                "\n[{}] FAILED: Verified swaps\n  Expected: >= {}\n  Actual: {}",
                Local::now().format("%H:%M:%S.%3f"),
                swaps_ok,
                stat.sign_counter
            );
            exit(1)
        } else {
            println!("[{}] PASSED: Verified swaps ({})", Local::now().format("%H:%M:%S.%3f"), stat.sign_counter);
        }
    }
    if let Some(best_profit) = test_config.assertions.best_profit_eth {
        if NWETH::from_float(best_profit) > stat.best_profit_eth {
            println!(
                "\n[{}] FAILED: Best profit\n  Expected: >= {} ETH\n  Actual: {} ETH",
                Local::now().format("%H:%M:%S.%3f"),
                best_profit,
                NWETH::to_float(stat.best_profit_eth)
            );
            exit(1)
        } else {
            println!("[{}] PASSED: Best profit ({} ETH)", Local::now().format("%H:%M:%S.%3f"), NWETH::to_float(stat.best_profit_eth));
        }
    }

    Ok(())
}
