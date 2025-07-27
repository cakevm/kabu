use crate::arguments::{AppArgs, Command, KabuArgs};
use alloy::eips::BlockId;
use alloy::providers::{IpcConnect, ProviderBuilder, WsConnect};
use alloy::rpc::client::ClientBuilder;
use clap::{CommandFactory, FromArgMatches, Parser};
use kabu::core::blockchain::{Blockchain, BlockchainState, Strategy};
use kabu::core::topology::TopologyConfig;
use kabu::evm::db::{AlloyDB, KabuDB};
use kabu::node::config::NodeBlockActorConfig;
use kabu::node::exex::mempool_worker;
use kabu::types::blockchain::KabuDataTypesEthereum;
use kabu_types_market::MarketState;
use reth::chainspec::{Chain, EthereumChainSpecParser};
use reth::cli::Cli;
use reth_node_ethereum::node::EthereumAddOns;
use reth_node_ethereum::EthereumNode;
use reth_provider::providers::BlockchainProvider;
use std::time::Duration;
use tokio::{signal, task};
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

mod arguments;
mod kabu_runtime;

fn main() -> eyre::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let fmt_layer = fmt::Layer::default().with_thread_ids(true).with_file(false).with_line_number(true).with_filter(env_filter);
    tracing_subscriber::registry().with(fmt_layer).init();

    // ignore arguments used by reth
    let app_args = AppArgs::from_arg_matches_mut(&mut AppArgs::command().ignore_errors(true).get_matches())?;
    match app_args.command {
        Command::Node(_) => Cli::<EthereumChainSpecParser, KabuArgs>::parse().run(|builder, kabu_args: KabuArgs| async move {
            let topology_config = TopologyConfig::load_from_file(kabu_args.kabu_config.clone())?;

            let bc = Blockchain::new(builder.config().chain.chain.id());
            let bc_clone = bc.clone();

            let handle = builder
                .with_types_and_provider::<EthereumNode, BlockchainProvider<_>>()
                .with_components(EthereumNode::components())
                .with_add_ons(EthereumAddOns::default())
                .install_exex("kabu-exex", |node_ctx| kabu_runtime::init(node_ctx, bc_clone, NodeBlockActorConfig::all_enabled()))
                .launch()
                .await?;

            let mempool = handle.node.pool.clone();
            let ipc_provider =
                ProviderBuilder::new().disable_recommended_fillers().connect_ipc(IpcConnect::new(handle.node.config.rpc.ipcpath)).await?;
            let alloy_db = AlloyDB::new(ipc_provider.clone(), BlockId::latest()).unwrap();

            let state_db = KabuDB::new().with_ext_db(alloy_db);

            let bc_state = BlockchainState::<KabuDB, KabuDataTypesEthereum>::new_with_market_state(MarketState::new(state_db));

            let strategy = Strategy::<KabuDB>::new();

            let bc_clone = bc.clone();
            tokio::task::spawn(async move {
                if let Err(e) = kabu_runtime::start_kabu(
                    ipc_provider,
                    bc_clone,
                    bc_state,
                    strategy,
                    topology_config,
                    kabu_args.kabu_config.clone(),
                    true,
                    None, // Task executor will be created internally
                )
                .await
                {
                    error!("Error starting kabu: {:?}", e);
                }
            });
            tokio::task::spawn(mempool_worker(mempool, bc));

            handle.node_exit_future.await
        }),
        Command::Remote(kabu_args) => {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

            rt.block_on(async {
                info!("Loading config from {}", kabu_args.kabu_config);
                let topology_config = TopologyConfig::load_from_file(kabu_args.kabu_config.clone())?;

                let client_config = topology_config.clients.get("remote").unwrap();
                let transport = WsConnect::new(client_config.url.clone());
                let client = ClientBuilder::default().ws(transport).await?;
                let provider = ProviderBuilder::new().disable_recommended_fillers().connect_client(client);
                let bc = Blockchain::new(Chain::mainnet().id());
                let bc_clone = bc.clone();

                let bc_state = BlockchainState::<KabuDB, KabuDataTypesEthereum>::new();

                let strategy = Strategy::<KabuDB>::new();

                if let Err(e) = kabu_runtime::start_kabu(
                    provider,
                    bc_clone,
                    bc_state,
                    strategy,
                    topology_config,
                    kabu_args.kabu_config.clone(),
                    false,
                    None,
                )
                .await
                {
                    error!("Error starting kabu: {:#?}", e);
                    panic!("{}", e)
                }

                // keep kabu running
                tokio::select! {
                    _ = signal::ctrl_c() => {
                    info!("CTRL+C received... exiting");
                }
                _ = async {
                        loop {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        task::yield_now().await;
                        }
                    } => {}
                }
                Ok::<(), eyre::Error>(())
            })?;
            Ok(())
        }
    }
}
