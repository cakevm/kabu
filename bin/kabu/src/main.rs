use crate::arguments::{AppArgs, Command, KabuArgs};
use alloy::eips::BlockId;
use alloy::network::Ethereum;
use alloy::primitives::hex;
use alloy::providers::{IpcConnect, ProviderBuilder, WsConnect};
use alloy::rpc::client::ClientBuilder;
use clap::{CommandFactory, FromArgMatches, Parser};
use diesel_migrations::{embed_migrations, EmbeddedMigrations};
use eyre::OptionExt;
use kabu::broadcast::accounts::InitializeSignersOneShotBlockingComponent;
use kabu::core::blockchain::{Blockchain, BlockchainState};
use kabu::core::components::{Component, KabuBuilder};
use kabu::core::topology::{EncoderConfig, TopologyConfig};
use kabu::defi::pools::PoolsLoadingConfig;
use kabu::evm::db::{AlloyDB, KabuDB};
use kabu::execution::multicaller::MulticallerSwapEncoder;
use kabu::node::debug_provider::DebugProviderExt;
use kabu::node::exex::mempool_worker;
use kabu::storage::db::init_db_pool_with_migrations;
use kabu::strategy::backrun::{BackrunConfig, BackrunConfigSection};
use kabu::types::blockchain::KabuDataTypesEthereum;
use kabu::types::entities::strategy_config::load_from_file;
use kabu_core_node::{KabuBuildContext, KabuEthereumNode};
use kabu_node_reth_api::{chain_notifications_forwarder, KabuRethFullProvider};
use kabu_types_market::{MarketState, PoolClass};
use reth::api::{NodeTypes, NodeTypesWithDBAdapter};
use reth::builder::NodeHandle;
use reth::chainspec::{Chain, EthereumChainSpecParser, MAINNET};
use reth::cli::Cli;
use reth::tasks::TaskManager;
use reth_db_api::database_metrics::DatabaseMetrics;
use reth_db_api::Database as RethDatabase;
use reth_node_ethereum::node::EthereumAddOns;
use reth_node_ethereum::EthereumNode;
use reth_provider::providers::BlockchainProvider;
use reth_storage_rpc_provider::{RpcBlockchainProvider, RpcBlockchainProviderConfig};
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

mod arguments;

// Embed migrations from the storage/db crate
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("../../migrations");

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
            let NodeHandle { node, node_exit_future } = builder
                .with_types_and_provider::<EthereumNode, BlockchainProvider<_>>()
                .with_components(EthereumNode::components())
                .with_add_ons(EthereumAddOns::default())
                .launch()
                .await?;

            let mempool = node.pool.clone();

            let ipc_provider =
                ProviderBuilder::new().disable_recommended_fillers().connect_ipc(IpcConnect::new(node.config.rpc.ipcpath)).await?;
            let alloy_db = AlloyDB::new(ipc_provider.clone(), BlockId::latest()).unwrap();
            let state_db = KabuDB::new().with_ext_db(alloy_db);
            let bc_state = BlockchainState::<KabuDB, KabuDataTypesEthereum>::new_with_market_state(MarketState::new(state_db));

            // Start Kabu MEV components
            let task_executor = node.task_executor.clone();

            let bc_clone = bc.clone();
            let reth_provider = node.provider.clone();
            let kabu_handle = tokio::task::spawn(async move {
                start_kabu_mev::<_, _, EthereumNode, _>(
                    reth_provider,
                    ipc_provider,
                    bc_clone,
                    bc_state,
                    topology_config,
                    kabu_args.kabu_config.clone(),
                    true,
                    task_executor,
                )
                .await
            });

            // Start mempool worker
            tokio::task::spawn(mempool_worker(mempool, bc));

            // Wait for either node exit or kabu handle
            tokio::select! {
                _ = node_exit_future => {
                    info!("Node exited");
                }
                result = kabu_handle => {
                    match result {
                        Ok(Ok(handle)) => {
                            info!("Kabu MEV components started, waiting for shutdown");
                            handle.wait_for_shutdown().await?;
                        }
                        Ok(Err(e)) => {
                            error!("Error starting kabu: {:?}", e);
                            return Err(e);
                        }
                        Err(e) => {
                            error!("Task panic: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
            }

            Ok(())
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
                let config = RpcBlockchainProviderConfig { compute_state_root: false, reth_rpc_support: false };
                let reth_provider = RpcBlockchainProvider::<_, EthereumNode, Ethereum>::new_with_config(provider.clone(), config)
                    .with_chain_spec(MAINNET.clone());

                let bc = Blockchain::new(Chain::mainnet().id());
                let bc_clone = bc.clone();

                let bc_state = BlockchainState::<KabuDB, KabuDataTypesEthereum>::new();

                let task_manager = TaskManager::new(tokio::runtime::Handle::current());
                let task_executor = task_manager.executor();

                chain_notifications_forwarder(&task_executor, provider.clone(), reth_provider.clone()).await?;

                let handle = start_kabu_mev::<_, _, EthereumNode, _>(
                    reth_provider,
                    provider,
                    bc_clone,
                    bc_state,
                    topology_config,
                    kabu_args.kabu_config.clone(),
                    false,
                    task_executor,
                )
                .await?;

                // Wait for shutdown
                handle.wait_for_shutdown().await?;
                Ok::<(), eyre::Error>(())
            })?;
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_kabu_mev<RethProvider, RpcProvider, Types, DB>(
    _reth_provider: RethProvider,
    provider: RpcProvider,
    bc: Blockchain,
    bc_state: BlockchainState<KabuDB, KabuDataTypesEthereum>,
    topology_config: TopologyConfig,
    kabu_config_filepath: String,
    is_exex: bool,
    task_executor: reth::tasks::TaskExecutor,
) -> eyre::Result<kabu::core::components::KabuHandle>
where
    RethProvider: KabuRethFullProvider<NodeTypesWithDBAdapter<Types, DB>>,
    Types: NodeTypes,
    DB: RethDatabase + DatabaseMetrics + Clone + Unpin + 'static,
    RpcProvider: alloy::providers::Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
{
    let chain_id = provider.get_chain_id().await?;
    info!(chain_id = ?chain_id, "Starting Kabu MEV bot");

    // Parse configuration
    let (_encoder_name, encoder) = topology_config.encoders.iter().next().ok_or_eyre("NO_ENCODER")?;
    let multicaller_address: alloy::primitives::Address = match encoder {
        EncoderConfig::SwapStep(e) => e.address.parse()?,
    };
    info!(address=?multicaller_address, "Multicaller");

    let db_url = topology_config.database.clone().unwrap().url;

    // Initialize database pool and handle migrations
    let db_pool = init_db_pool_with_migrations(db_url, MIGRATIONS).await?;

    let pools_config =
        PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default()).enable(PoolClass::UniswapV2).enable(PoolClass::UniswapV3);

    let backrun_config: BackrunConfigSection = load_from_file::<BackrunConfigSection>(kabu_config_filepath.into()).await?;
    let backrun_config: BackrunConfig = backrun_config.backrun_strategy;

    let swap_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);

    // Create KabuBuildContext
    let kabu_context = KabuBuildContext::builder(
        provider.clone(),
        bc,
        bc_state.clone(),
        topology_config.clone(),
        backrun_config.clone(),
        multicaller_address,
        Some(db_pool.clone()),
        is_exex,
    )
    .with_pools_config(pools_config.clone())
    .with_swap_encoder(swap_encoder.clone())
    .build();

    // Get references to channels before building
    let signers = kabu_context.channels.signers.clone();
    let account_state = kabu_context.channels.account_state.clone();

    // Build and launch MEV components using the compact builder pattern
    info!("Building MEV components using KabuBuilder with KabuEthereumNode");

    let handle = KabuBuilder::new(kabu_context)
        .node(KabuEthereumNode::<RpcProvider, KabuDB>::default())
        .build()
        .launch(task_executor.clone())
        .await?;

    // Initialize signers if DATA env var is provided
    if let Ok(key) = std::env::var("DATA") {
        info!("Initializing signers from DATA environment variable");
        let private_key_encrypted = hex::decode(key)?;
        let signer_initializer = InitializeSignersOneShotBlockingComponent::<KabuDataTypesEthereum>::new(Some(private_key_encrypted))
            .with_signers(signers)
            .with_monitor(account_state);

        signer_initializer.spawn(task_executor)?;
    } else {
        info!("No DATA environment variable found, skipping signer initialization");
    }

    Ok(handle)
}
