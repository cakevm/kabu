//! Default component builders that create standard Kabu components

use crate::context::KabuContext;
use crate::traits::{ComponentBuilder, DisabledComponent};
use alloy_network::Ethereum;
use alloy_provider::Provider;
use eyre::Result;
use std::sync::Arc;

use kabu_broadcast_broadcaster::FlashbotsBroadcastComponent;
use kabu_core_block_history::BlockHistoryComponent;
use kabu_core_components::Component;
use kabu_core_router::SwapRouterComponent;
use kabu_core_topology::BroadcasterConfig;
use kabu_defi_market::{HistoryPoolLoaderComponent, ProtocolPoolLoaderComponent};
use kabu_defi_pools::PoolLoadersBuilder;
use kabu_defi_preloader::MarketStatePreloadedOneShotComponent;
use kabu_defi_price::PriceComponent;
use kabu_evm_db::{DatabaseKabuExt, KabuDBError};
use kabu_execution_estimator::EvmEstimatorComponent;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_strategy_backrun::StateChangeArbComponent;
use kabu_strategy_merger::DiffPathMergerComponent;
use kabu_types_entities::BlockHistoryState;
use reth::revm::{Database, DatabaseCommit, DatabaseRef};
use reth_ethereum_primitives::EthPrimitives;

#[cfg(feature = "defi-health-monitor")]
use kabu_defi_health_monitor::PoolHealthMonitorComponent;

// ================================================================================================
// Network Component
// ================================================================================================

/// Default network builder that creates a BlockHistoryComponent
pub struct DefaultNetwork;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultNetwork
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = BlockHistoryComponent<P, Ethereum, DB, EthPrimitives>;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        Ok(BlockHistoryComponent::new(ctx.provider.clone()).with_channels(
            ctx.config.chain_params.clone(),
            ctx.market_state.clone(),
            ctx.block_history.clone(),
            ctx.blockchain.new_block_headers_channel(),
            ctx.blockchain.new_block_with_tx_channel(),
            ctx.blockchain.new_block_state_update_channel(),
            ctx.channels.market_events.clone(),
        ))
    }
}

// ================================================================================================
// Executor Component
// ================================================================================================

/// Default executor builder that creates a SwapRouterComponent
pub struct DefaultExecutor;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultExecutor
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = SwapRouterComponent<DB, EthPrimitives>;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        Ok(SwapRouterComponent::new(ctx.channels.signers.clone(), ctx.channels.account_state.clone(), ctx.channels.swap_compose.clone()))
    }
}

// ================================================================================================
// Strategy Component
// ================================================================================================

/// Default strategy builder that creates a StateChangeArbComponent
pub struct DefaultStrategy;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultStrategy
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = StateChangeArbComponent<P, Ethereum, DB, EthPrimitives>;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        let mut component = StateChangeArbComponent::new(
            ctx.provider.clone(),
            true,                // use_blocks
            !ctx.config.is_exex, // use_mempool (only if not exex)
            ctx.config.backrun_config.clone(),
        )
        .with_market(ctx.market.clone())
        .with_mempool(ctx.mempool.clone())
        .with_market_state(ctx.market_state.clone())
        .with_block_history(ctx.block_history.clone())
        .with_mempool_events_channel(ctx.channels.mempool_events.clone())
        .with_market_events_channel(ctx.channels.market_events.clone())
        .with_swap_compose_channel(ctx.channels.swap_compose.clone())
        .with_pool_health_monitor_channel(ctx.channels.health_events.clone());

        if let Some(channel) = ctx.blockchain.influxdb_write_channel() {
            component = component.with_influxdb_channel(channel);
        }

        Ok(component)
    }
}

// ================================================================================================
// Signer Bridge Component
// ================================================================================================

/// Default signer bridge builder
pub struct DefaultSigner;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultSigner
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = crate::signer::SignerBridgeComponent<DB>;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        Ok(crate::signer::SignerBridgeComponent::new().with_channels(ctx.channels.swap_compose.clone(), ctx.channels.tx_compose.clone()))
    }
}

// ================================================================================================
// Market Components (Composite)
// ================================================================================================

/// Composite component that combines all market-related components
#[derive(Clone)]
pub struct MarketComponents<P, DB>
where
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: Send + Sync + Clone + 'static,
{
    preloader: MarketStatePreloadedOneShotComponent<P, Ethereum, DB>,
    price: PriceComponent<P, Ethereum>,
    protocol_loader: ProtocolPoolLoaderComponent<P, P, Ethereum>,
    history_loader: HistoryPoolLoaderComponent<P, P, Ethereum>,
}

impl<P, DB> Component for MarketComponents<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        self.preloader.spawn(executor.clone())?;
        self.price.spawn(executor.clone())?;
        self.protocol_loader.spawn(executor.clone())?;
        self.history_loader.spawn(executor)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MarketComponents"
    }
}

/// Default market builder
pub struct DefaultMarket;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultMarket
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = MarketComponents<P, DB>;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::<_, _, EthPrimitives>::default_pool_loaders(
            ctx.provider.clone(),
            ctx.config.pools_config.clone(),
        ));

        let preloader = MarketStatePreloadedOneShotComponent::new(ctx.provider.clone())
            .with_copied_account(ctx.config.swap_encoder.get_contract_address())
            .with_signers(ctx.channels.signers.clone())
            .with_market_state(ctx.market_state.clone());

        let price = PriceComponent::new(ctx.provider.clone()).only_once().with_market(ctx.market.clone());

        let protocol_loader = ProtocolPoolLoaderComponent::new(ctx.provider.clone(), pool_loaders.clone());

        let history_loader = HistoryPoolLoaderComponent::new(
            ctx.provider.clone(),
            pool_loaders,
            0,    // start_block
            1000, // block_batch_size
            10,   // max_batches
        );

        Ok(MarketComponents { preloader, price, protocol_loader, history_loader })
    }
}

// ================================================================================================
// Broadcaster Component
// ================================================================================================

/// Default broadcaster builder
pub struct DefaultBroadcaster;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultBroadcaster
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = FlashbotsBroadcastComponent;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        // Get flashbots relays from config
        let relays = ctx
            .config
            .topology_config
            .actors
            .broadcaster
            .as_ref()
            .and_then(|b| b.get("mainnet"))
            .map(|b| match b {
                BroadcasterConfig::Flashbots(f) => f.relays(),
            })
            .unwrap_or_default();

        let mut component = FlashbotsBroadcastComponent::new(None, !relays.is_empty())?;
        component = component.with_relays(relays)?;
        component = component.with_channel(ctx.channels.tx_compose.clone());
        Ok(component)
    }
}

// ================================================================================================
// Estimator Component
// ================================================================================================

/// Default estimator builder
pub struct DefaultEstimator;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultEstimator
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = EvmEstimatorComponent<P, Ethereum, kabu_execution_multicaller::MulticallerSwapEncoder, DB>;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        Ok(EvmEstimatorComponent::new_with_provider(ctx.config.swap_encoder.clone(), Some(ctx.provider.clone()))
            .with_swap_compose_channel(ctx.channels.swap_compose.clone()))
    }
}

// ================================================================================================
// Health Monitor Component
// ================================================================================================

/// Default health monitor builder
pub struct DefaultHealthMonitor;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultHealthMonitor
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    #[cfg(feature = "defi-health-monitor")]
    type Component = PoolHealthMonitorComponent;

    #[cfg(not(feature = "defi-health-monitor"))]
    type Component = DisabledComponent;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        #[cfg(feature = "defi-health-monitor")]
        {
            Ok(PoolHealthMonitorComponent::new().with_channels(
                ctx.market.clone(),
                ctx.channels.health_events.clone(),
                ctx.blockchain.influxdb_write_channel(),
            ))
        }

        #[cfg(not(feature = "defi-health-monitor"))]
        {
            let _ = ctx;
            Ok(DisabledComponent)
        }
    }
}

// ================================================================================================
// Merger Components (Composite)
// ================================================================================================

/// Composite merger components
#[derive(Clone)]
pub struct MergerComponents<DB>
where
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
{
    diff_path: DiffPathMergerComponent<DB>,
}

impl<DB> Component for MergerComponents<DB>
where
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
{
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        self.diff_path.spawn(executor)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MergerComponents"
    }
}

/// Default merger builder
pub struct DefaultMerger;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultMerger
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = MergerComponents<DB>;

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        let diff_path = DiffPathMergerComponent::<DB>::new()
            .with_market_events_channel(ctx.channels.market_events.clone())
            .with_compose_channel(ctx.channels.swap_compose.clone());

        Ok(MergerComponents { diff_path })
    }
}

// ================================================================================================
// Monitoring Component
// ================================================================================================

/// Default monitoring builder
pub struct DefaultMonitoring;

impl<R, P, DB, Evm> ComponentBuilder<KabuContext<R, P, DB, Evm>> for DefaultMonitoring
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    type Component = DisabledComponent; // Or InfluxDbWriterComponent if configured

    fn build(self, ctx: &KabuContext<R, P, DB, Evm>) -> Result<Self::Component> {
        // For now, return disabled. Could check config for InfluxDB settings
        let _ = ctx;
        Ok(DisabledComponent)
    }
}
