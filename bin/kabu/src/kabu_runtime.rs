use alloy::network::Ethereum;
use alloy::primitives::Address;
use alloy::providers::Provider;
use eyre::OptionExt;
use kabu::core::blockchain::{Blockchain, BlockchainState, Strategy};
use kabu::core::components::BuilderContext;
use kabu::core::topology::{EncoderConfig, TopologyConfig};
use kabu::defi::pools::PoolsLoadingConfig;
use kabu::evm::db::KabuDB;
use kabu::execution::multicaller::MulticallerSwapEncoder;
use kabu::node::config::NodeBlockComponentConfig;
use kabu::node::debug_provider::DebugProviderExt;
use kabu::node::exex::kabu_exex;
use kabu::storage::db::init_db_pool;
use kabu::strategy::backrun::{BackrunConfig, BackrunConfigSection};
use kabu::types::blockchain::KabuDataTypesEthereum;
use kabu::types::entities::strategy_config::load_from_file;
use kabu_core_components::Component;
use kabu_types_market::PoolClass;
use reth::api::NodeTypes;
use reth::tasks::{TaskExecutor, TaskManager};
use reth_exex::ExExContext;
use reth_node_api::FullNodeComponents;
use reth_primitives::EthPrimitives;
use std::future::Future;
use tracing::{error, info};

use kabu::core::components::{MergerBuilder, MonitoringBuilder, WebServerBuilder};
use kabu_core_node::{KabuBuildContext, KabuMergerBuilder, KabuMonitoringBuilder, KabuNode, KabuWebServerBuilder};

pub async fn init<Node>(
    ctx: ExExContext<Node>,
    bc: Blockchain,
    config: NodeBlockComponentConfig,
) -> eyre::Result<impl Future<Output = eyre::Result<()>>>
where
    Node: FullNodeComponents<Types: NodeTypes<Primitives = EthPrimitives>>,
{
    Ok(kabu_exex(ctx, bc, config.clone()))
}

#[allow(clippy::too_many_arguments)]
pub async fn start_kabu<P>(
    provider: P,
    bc: Blockchain,
    bc_state: BlockchainState<KabuDB, KabuDataTypesEthereum>,
    _strategy: Strategy<KabuDB>,
    topology_config: TopologyConfig,
    kabu_config_filepath: String,
    is_exex: bool,
    task_executor: Option<TaskExecutor>,
) -> eyre::Result<()>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
{
    let chain_id = provider.get_chain_id().await?;
    info!(chain_id = ?chain_id, "Starting Kabu MEV bot");

    // Parse configuration
    let (_encoder_name, encoder) = topology_config.encoders.iter().next().ok_or_eyre("NO_ENCODER")?;
    let multicaller_address: Address = match encoder {
        EncoderConfig::SwapStep(e) => e.address.parse()?,
    };
    // Note: Private key handling is now done by the market builder via the DATA env var
    info!(address=?multicaller_address, "Multicaller");

    let _webserver_host = topology_config.webserver.clone().unwrap_or_default().host;
    let db_url = topology_config.database.clone().unwrap().url;
    let db_pool = init_db_pool(db_url).await?;

    // Note: Flashbots relays are already handled by the broadcaster builder

    let pools_config =
        PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default()).enable(PoolClass::UniswapV2).enable(PoolClass::UniswapV3);

    let backrun_config: BackrunConfigSection = load_from_file::<BackrunConfigSection>(kabu_config_filepath.into()).await?;
    let backrun_config: BackrunConfig = backrun_config.backrun_strategy;

    let swap_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);

    // Use provided task executor or create a new one
    let executor = match task_executor {
        Some(executor) => executor,
        None => {
            let task_manager = TaskManager::new(tokio::runtime::Handle::current());
            task_manager.executor()
        }
    };

    // Create KabuBuildContext with defaults or use builder for customization
    let kabu_context = KabuBuildContext::builder(
        provider.clone(),
        bc,
        bc_state.clone(),
        topology_config.clone(),
        backrun_config.clone(),
        multicaller_address,
        db_pool.clone(),
        is_exex,
    )
    .with_pools_config(pools_config.clone())
    .with_swap_encoder(swap_encoder.clone())
    .build();

    // Access MEV channels from the context
    let mev_channels = kabu_context.channels.clone();

    // Create builder context
    let builder_context = BuilderContext::with_channels(kabu_context, mev_channels.clone());

    // Note: Initialize signers is now handled by the market builder

    // Wait for node sync if needed
    if !is_exex {
        // In exex mode, we're already synced
        // TODO: Implement wait for sync logic if needed
    }

    // Build and spawn all MEV components using the builder pattern
    info!("Building MEV components using Kabu node builders");
    let components = KabuNode::components::<P, KabuDB>();
    match components.build_and_spawn(builder_context.clone(), executor.clone()).await {
        Ok(_) => info!("All core MEV components built and spawned successfully"),
        Err(e) => {
            error!("Failed to build MEV components: {}", e);
            return Err(e);
        }
    }

    // Build and spawn merger components
    info!("Building merger components");
    let merger_builder = KabuMergerBuilder::<P, KabuDB>::new();
    match merger_builder.build_merger(&builder_context).await {
        Ok(merger) => {
            merger.spawn(executor.clone())?;
            info!("Merger components built and spawned successfully");
        }
        Err(e) => {
            error!("Failed to build merger components: {}", e);
            return Err(e);
        }
    }

    // Build and spawn web server component
    info!("Building web server component");
    let web_server_builder = KabuWebServerBuilder::<P, KabuDB>::new();
    match web_server_builder.build_web_server(&builder_context).await {
        Ok(web_server) => {
            web_server.spawn(executor.clone())?;
            info!("Web server component built and spawned successfully");
        }
        Err(e) => {
            error!("Failed to build web server component: {}", e);
            return Err(e);
        }
    }

    // Build and spawn monitoring components
    info!("Building monitoring components");
    let monitoring_builder = KabuMonitoringBuilder::<P, KabuDB>::new();
    match monitoring_builder.build_monitoring(&builder_context).await {
        Ok(monitoring) => {
            monitoring.spawn(executor.clone())?;
            info!("Monitoring components built and spawned successfully");
        }
        Err(e) => {
            error!("Failed to build monitoring components: {}", e);
            return Err(e);
        }
    }

    // Note: Market builder already handles initialization, market state preload, price,
    // account monitor, and pool loaders
    // Note: Network builder already handles block processing
    // Note: Merger, web server, and monitoring components are spawned separately above

    info!("All MEV components started successfully");

    // Keep runtime alive
    tokio::signal::ctrl_c().await?;
    info!("Shutting down Kabu MEV bot");
    Ok(())
}
