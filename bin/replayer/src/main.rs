use kabu_evm_db::KabuDB;
use std::env;
use std::process::exit;
use std::time::Duration;

use alloy::network::TransactionBuilder;
use alloy::primitives::{address, Address, U256};
use alloy::providers::Provider;
use alloy::rpc::types::Header;
use alloy::{providers::ProviderBuilder, rpc::client::ClientBuilder};
use alloy_evm::EvmEnv;
use clap::Parser;
use eyre::Result;
use tokio::select;
use url::Url;

use kabu_node_debug_provider::HttpCachedTransport;

use kabu_core_blockchain::{Blockchain, BlockchainState, Strategy};
use kabu_core_blockchain_actors::BlockchainActors;
use kabu_defi_abi::AbiEncoderHelper;
use kabu_defi_address_book::{TokenAddressEth, UniswapV3PoolAddress};
use kabu_defi_pools::state_readers::ERC20StateReader;
use kabu_evm_db::DatabaseKabuExt;
use kabu_evm_utils::NWETH;
use kabu_execution_multicaller::MulticallerSwapEncoder;
use kabu_node_player::NodeBlockPlayerActor;
use kabu_types_entities::required_state::RequiredState;
use kabu_types_entities::{MarketState, PoolClass, PoolId, Swap, SwapAmountType, SwapLine};
use kabu_types_events::{MessageSwapCompose, SwapComposeData, TxComposeData};
use tracing::{debug, error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

#[derive(Parser, Debug)]
struct Commands {
    /// Run replayer for the given block number count
    #[arg(short, long)]
    terminate_after_block_count: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_block_number = 20179184;

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "debug,alloy_rpc_client=off,kabu_node_debug_provider=info,alloy_transport_http=off,hyper_util=off,kabu_core_block_history=trace"
            .into()
    });
    let fmt_layer = fmt::Layer::default().with_thread_ids(true).with_file(false).with_line_number(true).with_filter(env_filter);

    tracing_subscriber::registry().with(fmt_layer).init();

    let args = Commands::parse();
    let node_url = env::var("MAINNET_HTTP")?;
    let node_url = Url::parse(node_url.as_str())?;

    let transport = HttpCachedTransport::new(node_url.clone(), Some("./.cache")).await;
    transport.set_block_number(start_block_number);

    let client = ClientBuilder::default().transport(transport.clone(), true).with_poll_interval(Duration::from_millis(50));
    let provider = ProviderBuilder::new().disable_recommended_fillers().connect_client(client);

    let node_provider = ProviderBuilder::new().disable_recommended_fillers().connect_http(node_url);

    // creating singers
    //let tx_signers = SharedState::new(TxSigners::new());

    // new blockchain
    let bc = Blockchain::new(1);

    let bc_state = BlockchainState::new_with_market_state(MarketState::new(KabuDB::empty()));

    let market_state = bc_state.market_state();

    let strategy = Strategy::<KabuDB>::new();

    let swap_encoder = MulticallerSwapEncoder::default();

    const TARGET_ADDRESS: Address = address!("A69babEF1cA67A37Ffaf7a485DfFF3382056e78C");

    let mut required_state = RequiredState::new();
    required_state.add_call(TokenAddressEth::WETH, AbiEncoderHelper::encode_erc20_balance_of(TARGET_ADDRESS));

    // instead fo code above
    let mut bc_actors =
        BlockchainActors::new(provider.clone(), swap_encoder.clone(), bc.clone(), bc_state.clone(), strategy.clone(), vec![]);
    bc_actors
        .with_nonce_and_balance_monitor_only_once()?
        .with_signers()?
        .with_market_state_preloader_virtual(vec![])?
        .with_preloaded_state(vec![(UniswapV3PoolAddress::USDC_WETH_500, PoolClass::UniswapV3)], Some(required_state))?
        .with_block_history()?
        .with_swap_encoder(swap_encoder)?
        .with_evm_estimator()?;

    //Start node block player actor
    if let Err(e) =
        bc_actors.start(NodeBlockPlayerActor::new(provider.clone(), start_block_number, start_block_number + 200).on_bc(&bc, &bc_state))
    {
        panic!("Cannot start block player : {e}");
    }

    tokio::task::spawn(bc_actors.wait());
    let compose_channel = strategy.swap_compose_channel();

    let mut header_sub = bc.new_block_headers_channel().subscribe();
    let mut block_sub = bc.new_block_with_tx_channel().subscribe();
    let mut logs_sub = bc.new_block_logs_channel().subscribe();
    let mut state_update_sub = bc.new_block_state_update_channel().subscribe();

    //let memepool = bc.mempool();
    let market = bc.market();

    let mut cur_header: Header = Header::default();

    loop {
        select! {
            header = header_sub.recv() => {
                match header {
                    Ok(message_header)=>{
                        let header = message_header.inner.header;
                        info!("Block header received: block_number={}, block_hash={}", header.number, header.hash);

                        if let Some(terminate_after_block_count) = args.terminate_after_block_count {
                            println!("Replay current_block={}/{}", header.number, start_block_number + terminate_after_block_count);
                            if header.number >= start_block_number + terminate_after_block_count {
                                println!("Successful for start_block_number={}, current_block={}, terminate_after_block_count={}", start_block_number, header.number, terminate_after_block_count);
                                exit(0);
                            }
                        }

                        cur_header = header.clone();
                        if header.number % 10 == 0 {
                            info!("Composing swap: block_number={}, block_hash={}", header.number, header.hash);

                            let swap_path = market.read().await.swap_path(vec![TokenAddressEth::WETH, TokenAddressEth::USDC], vec![PoolId::Address(UniswapV3PoolAddress::USDC_WETH_500)])?;
                            let mut swap_line = SwapLine::from(swap_path);
                            swap_line.amount_in = SwapAmountType::Set( NWETH::from_float(0.1));
                            swap_line.gas_used = Some(300000);

                            let tx_compose_encode_msg = MessageSwapCompose::prepare(
                                SwapComposeData{
                                    tx_compose : TxComposeData {
                                        next_block_base_fee : bc.chain_parameters().calc_next_block_base_fee_from_header(&header),
                                        ..TxComposeData::default()
                                    },
                                    poststate : Some(market_state.read().await.state_db.clone()),
                                    swap : Swap::ExchangeSwapLine(swap_line),
                                    ..SwapComposeData::default()
                                });

                            if let Err(e) = compose_channel.send(tx_compose_encode_msg) {
                                error!("compose_channel.send : {}", e)
                            }else{
                                debug!("compose_channel.send ok");
                            }
                        }
                    }
                    Err(e)=>{
                        error!("Error receiving headers: {e}");
                    }
                }
            }

            logs = logs_sub.recv() => {
                match logs{
                    Ok(logs_update)=>{
                        info!("Block logs received : {} log records : {}", logs_update.block_header.hash, logs_update.logs.len());
                    }
                    Err(e)=>{
                        error!("Error receiving logs: {e}");
                    }
                }
            }

            block = block_sub.recv() => {
                match block {
                    Ok(block_msg)=>{
                        info!("Block with tx received : {} txs : {}", block_msg.block.header.hash, block_msg.block.transactions.len());
                    }
                    Err(e)=>{
                        error!("Error receiving blocks: {e}");
                    }
                }
            }
            state_udpate = state_update_sub.recv() => {
                match state_udpate {
                    Ok(state_update)=>{
                        let state_update = state_update.inner;

                        info!("Block state update received : {} update records : {}", state_update.block_header.hash, state_update.state_update.len() );
                        let mut state_db = market_state.read().await.state_db.clone();
                        state_db.apply_geth_update_vec(state_update.state_update);


                        // Note: ERC20StateReader calls will still fail until full EVM integration is complete,
                        // but database access is now direct
                        if let Ok(balance) = ERC20StateReader::balance_of(&state_db, &EvmEnv::default(), TARGET_ADDRESS, TokenAddressEth::WETH ) {
                            info!("------WETH Balance of {} : {}", TARGET_ADDRESS, balance);
                            let tx_req = alloy::rpc::types::TransactionRequest::default()
                                .to(TokenAddressEth::WETH)
                                .with_input(AbiEncoderHelper::encode_erc20_balance_of(TARGET_ADDRESS));
                            let fetched_balance = node_provider.call(tx_req).block(cur_header.number.into()).await?;

                            let fetched_balance = U256::from_be_slice(fetched_balance.to_vec().as_slice());
                            if fetched_balance != balance {
                                error!("Balance is wrong {}/({:#x}) need {}({:#x})", balance, balance, fetched_balance, fetched_balance);
                                exit(1);
                            }
                        } else {
                            info!("ERC20StateReader call failed - EVM integration not complete yet");
                        }
                        if let Ok(balance) = ERC20StateReader::balance_of(&state_db, &EvmEnv::default(), UniswapV3PoolAddress::USDC_WETH_500, TokenAddressEth::WETH ) {
                            info!("------WETH Balance of {} : {}/({:#x}) ", UniswapV3PoolAddress::USDC_WETH_500, balance, balance);
                        } else {
                            info!("ERC20StateReader call for pool balance failed - EVM integration not complete yet");
                        }

                        info!("StateDB : Accounts: {} / {} Contracts : {} / {}", state_db.accounts_len(), state_db.ro_accounts_len(), state_db.contracts_len(), state_db.ro_contracts_len())

                    }
                    Err(e)=>{
                        error!("Error receiving blocks: {e}");
                    }
                }
            }
        }
    }
}
