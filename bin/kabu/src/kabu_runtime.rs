use alloy::network::Ethereum;
use alloy::primitives::Address;
use alloy::providers::Provider;
use eyre::OptionExt;
use kabu::core::blockchain::{AppState, Blockchain, BlockchainState, EventChannels, Strategy};
use kabu::core::components::{BuilderContext, KabuRuntime, RuntimeState};
use kabu::core::topology::{BroadcasterConfig, EncoderConfig, TopologyConfig};
use kabu::defi::pools::PoolsLoadingConfig;
use kabu::evm::db::{DatabaseKabuExt, KabuDBError};
use kabu::execution::multicaller::MulticallerSwapEncoder;
use kabu::kabu_node::KabuNode;
use kabu::node::config::NodeBlockActorConfig;
use kabu::node::debug_provider::DebugProviderExt;
use kabu::node::exex::kabu_exex;
use kabu::storage::db::init_db_pool;
use kabu::strategy::backrun::{BackrunConfig, BackrunConfigSection};
use kabu::types::blockchain::KabuDataTypesEthereum;
use kabu::types::entities::strategy_config::load_from_file;
use kabu::types::entities::BlockHistoryState;
use kabu_types_market::PoolClass;
use reth::api::NodeTypes;
use reth::revm::{Database, DatabaseCommit, DatabaseRef};
use reth::tasks::{TaskExecutor, TaskManager};
use reth_exex::ExExContext;
use reth_node_api::FullNodeComponents;
use reth_primitives::EthPrimitives;
use std::env;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub async fn init<Node>(
    ctx: ExExContext<Node>,
    bc: Blockchain,
    config: NodeBlockActorConfig,
) -> eyre::Result<impl Future<Output = eyre::Result<()>>>
where
    Node: FullNodeComponents<Types: NodeTypes<Primitives = EthPrimitives>>,
{
    Ok(kabu_exex(ctx, bc, config.clone()))
}

#[allow(clippy::too_many_arguments)]
pub async fn start_kabu<P, DB>(
    provider: P,
    _bc: Blockchain,
    bc_state: BlockchainState<DB, KabuDataTypesEthereum>,
    strategy: Strategy<DB>,
    topology_config: TopologyConfig,
    kabu_config_filepath: String,
    is_exex: bool,
    task_executor: Option<TaskExecutor>,
) -> eyre::Result<()>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: Database<Error = KabuDBError>
        + DatabaseRef<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    let chain_id = provider.get_chain_id().await?;
    info!(chain_id = ?chain_id, "Starting Kabu with component architecture");

    // Parse configuration
    let (_encoder_name, encoder) = topology_config.encoders.iter().next().ok_or_eyre("NO_ENCODER")?;
    let multicaller_address: Address = match encoder {
        EncoderConfig::SwapStep(e) => e.address.parse()?,
    };
    let _private_key_encrypted = hex::decode(env::var("DATA")?)?;
    info!(address=?multicaller_address, "Multicaller");

    let _webserver_host = topology_config.webserver.unwrap_or_default().host;
    let db_url = topology_config.database.unwrap().url;
    let _db_pool = init_db_pool(db_url).await?;

    // Get flashbots relays from config
    let _relays = topology_config
        .actors
        .broadcaster
        .as_ref()
        .and_then(|b| b.get("mainnet"))
        .map(|b| match b {
            BroadcasterConfig::Flashbots(f) => f.relays(),
        })
        .unwrap_or_default();

    let _pools_config =
        PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default()).enable(PoolClass::UniswapV2).enable(PoolClass::UniswapV3);

    let backrun_config: BackrunConfigSection = load_from_file::<BackrunConfigSection>(kabu_config_filepath.into()).await?;
    let _backrun_config: BackrunConfig = backrun_config.backrun_strategy;

    let swap_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);

    // Convert old Blockchain to new AppState and EventChannels
    let app_state = AppState::new(chain_id);

    let mut channels = EventChannels::default();
    if topology_config.influxdb.is_some() {
        channels = channels.with_influxdb();
    }

    // Create runtime state
    let runtime_state = RuntimeState {
        provider: provider.clone(),
        app_state: app_state.clone(),
        blockchain_state: bc_state.clone(),
        channels: channels.clone(),
        strategy: strategy.clone(),
        signers: Arc::new(RwLock::new(kabu::types::entities::TxSigners::new())),
        encoder: swap_encoder.clone(),
    };

    // Use provided task executor or create a new one
    let executor = match task_executor {
        Some(executor) => executor,
        None => {
            let task_manager = TaskManager::new(tokio::runtime::Handle::current());
            task_manager.executor()
        }
    };

    let runtime = KabuRuntime::new(runtime_state.clone(), executor);

    // Build MEV bot components using the new architecture
    let mev_components = KabuNode::components();

    // Build and spawn all components
    let ctx = BuilderContext::new(runtime.state().clone());
    mev_components.build_and_spawn(ctx, runtime.executor().clone()).await?;

    // Add additional components based on configuration
    if !is_exex {
        // Add remote mempool monitoring
        // runtime.component_manager().spawn(RemoteMempoolComponent::new(provider.clone()));
    }

    if let Some(_influxdb_config) = topology_config.influxdb {
        // Add metrics recording
        // runtime.component_manager().spawn(
        //     MetricsComponent::new(influxdb_config.url, influxdb_config.database, influxdb_config.tags)
        // );
    }

    // Keep runtime alive
    // TODO: Implement proper shutdown mechanism
    tokio::signal::ctrl_c().await?;
    Ok(())
}

// Example of how the old actor-based methods map to new component builders
//
// Old:
// bc_actors
//     .mempool()?
//     .with_wait_for_node_sync()?
//     .initialize_signers_with_encrypted_key(key)?
//     .with_block_history()?
//     .with_price_station()?
//     .with_health_monitor_pools()?
//     .with_swap_encoder(encoder)?
//     .with_evm_estimator()?
//     .with_signers()?
//     .with_flashbots_broadcaster(true)?
//     .with_market_state_preloader()?
//     .with_pool_loader(pools_config)?
//     .with_backrun_block(backrun_config)?
//
// New:
// MevBotComponentsBuilder::new()
//     .pool_loaders(PoolLoaderBuilder::new().with_all_default())
//     .price_monitor(PriceMonitorBuilder::new())
//     .block_monitor(BlockMonitorBuilder::new().all_enabled())
//     .strategy_runner(StrategyRunnerBuilder::new().with_backrun(max_gas))
//     .signer(SignerBuilder::new().with_key(key).with_flashbots())
//     .health_monitor(HealthMonitorBuilder::new().with_pool_monitoring())
