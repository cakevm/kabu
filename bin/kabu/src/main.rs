use crate::arguments::{AppArgs, Command, KabuArgs};
use alloy::eips::BlockId;
use alloy::network::Ethereum;
use alloy::primitives::hex;
use alloy::providers::{IpcConnect, Provider, ProviderBuilder, WsConnect};
use alloy::rpc::client::ClientBuilder;
use clap::{CommandFactory, FromArgMatches, Parser};
use diesel_migrations::{EmbeddedMigrations, embed_migrations};
use eyre::OptionExt;
use kabu::broadcast::accounts::InitializeSignersOneShotBlockingComponent;
use kabu::core::blockchain::{Blockchain, BlockchainState};
use kabu::core::components::Component;
use kabu::core::topology::{EncoderConfig, TopologyConfig};
use kabu::defi::pools::PoolsLoadingConfig;
use kabu::evm::db::{AlloyDB, KabuDB};
use kabu::execution::multicaller::MulticallerSwapEncoder;
use kabu::node::debug_provider::DebugProviderExt;
use kabu::node::exex::mempool_worker;
use kabu::storage::db::init_db_pool_with_migrations;
use kabu::strategy::backrun::{BackrunConfig, BackrunConfigSection};
use kabu::types::entities::strategy_config::load_from_file;
use kabu_core_node::{KabuContext, KabuEthereumNode, KabuHandle, NodeBuilder, NodeConfig};
use kabu_node_reth_api::{KabuRethFullProvider, chain_notifications_forwarder};
use kabu_types_market::{MarketState, PoolClass};
use reth::api::{NodeTypes, NodeTypesWithDBAdapter};
use reth::builder::NodeHandle;
use reth::chainspec::{Chain, EthereumChainSpecParser, MAINNET};
use reth::cli::Cli;
use reth::tasks::{TaskExecutor, TaskManager};
use reth_db_api::Database as RethDatabase;
use reth_db_api::database_metrics::DatabaseMetrics;
use reth_ethereum_primitives::EthPrimitives;
use reth_evm::ConfigureEvm;
use reth_node_ethereum::node::EthereumAddOns;
use reth_node_ethereum::{EthEvmConfig, EthereumNode};
use reth_provider::providers::BlockchainProvider;
use reth_storage_rpc_provider::{RpcBlockchainProvider, RpcBlockchainProviderConfig};
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

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
            let bc_state = BlockchainState::<KabuDB, EthPrimitives>::new_with_market_state(MarketState::new(state_db));

            // Start Kabu MEV components
            let task_executor = node.task_executor.clone();
            let bc_clone = bc.clone();
            let reth_provider = node.provider.clone();
            let evm_config = node.evm_config.clone();
            let kabu_handle = tokio::task::spawn(async move {
                start_kabu_mev::<_, _, EthereumNode, _, _>(
                    reth_provider,
                    ipc_provider,
                    evm_config,
                    bc_clone,
                    bc_state,
                    topology_config,
                    kabu_args.kabu_config.clone(),
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
                let rpc_provider = ProviderBuilder::new().disable_recommended_fillers().connect_client(client);
                let config = RpcBlockchainProviderConfig { compute_state_root: false, reth_rpc_support: false };
                let reth_provider = RpcBlockchainProvider::<_, EthereumNode, Ethereum>::new_with_config(rpc_provider.clone(), config)
                    .with_chain_spec(MAINNET.clone());

                let bc = Blockchain::new(Chain::mainnet().id());
                let bc_state = BlockchainState::<KabuDB, EthPrimitives>::new();

                let task_manager = TaskManager::new(tokio::runtime::Handle::current());
                let task_executor = task_manager.executor();

                chain_notifications_forwarder(&task_executor, rpc_provider.clone(), reth_provider.clone()).await?;

                let evm_config = EthEvmConfig::mainnet();

                let handle = start_kabu_mev::<_, _, EthereumNode, _, _>(
                    reth_provider,
                    rpc_provider,
                    evm_config,
                    bc,
                    bc_state,
                    topology_config,
                    kabu_args.kabu_config.clone(),
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
async fn start_kabu_mev<RethProvider, RpcProvider, Types, DB, EvmConfig>(
    reth_provider: RethProvider,
    rpc_provider: RpcProvider,
    evm_config: EvmConfig,
    bc: Blockchain,
    bc_state: BlockchainState<KabuDB, EthPrimitives>,
    topology_config: TopologyConfig,
    kabu_config_filepath: String,
    task_executor: TaskExecutor,
) -> eyre::Result<KabuHandle>
where
    RethProvider: KabuRethFullProvider<NodeTypesWithDBAdapter<Types, DB>>,
    Types: NodeTypes<Primitives = EthPrimitives>,
    DB: RethDatabase + DatabaseMetrics + Clone + Unpin + 'static,
    RpcProvider: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    EvmConfig: ConfigureEvm + 'static,
{
    let chain_id = rpc_provider.get_chain_id().await?;
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

    // Create node configuration
    let config = NodeConfig::new()
        .chain_id(chain_id)
        .topology_config(topology_config.clone())
        .backrun_config(backrun_config.clone())
        .multicaller_address(multicaller_address)
        .swap_encoder(swap_encoder.clone())
        .pools_config(pools_config.clone())
        .db_pool(db_pool.clone());

    // Create context with all providers
    let context = KabuContext::new(reth_provider, rpc_provider.clone(), evm_config, bc, bc_state.clone(), config);

    // Get references to channels before launching
    let signers = context.channels.signers.clone();
    let account_state = context.channels.account_state.clone();

    // Build and launch MEV components
    info!("Building MEV components with new node architecture");

    let handle = NodeBuilder::new()
        .with_context(context)
        .with_components(KabuEthereumNode::components().build())
        .launch(task_executor.clone())
        .await?;

    // Initialize signers if DATA env var is provided
    if let Ok(key) = std::env::var("DATA") {
        info!("Initializing signers from DATA environment variable");
        let private_key_encrypted = hex::decode(key)?;
        let signer_initializer = InitializeSignersOneShotBlockingComponent::<EthPrimitives>::new(Some(private_key_encrypted))
            .with_signers(signers)
            .with_monitor(account_state);

        signer_initializer.spawn(task_executor)?;
    } else {
        info!("No DATA environment variable found, skipping signer initialization");
    }

    Ok(handle)
}
