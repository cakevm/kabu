//! Kabu MEV Bot Node Implementation
//!
//! This module provides component builders following the Reth architecture pattern
//! for future node integration. Currently, only ExecutorBuilder and BroadcasterBuilder
//! are fully implemented. Other builders return PlaceholderComponent as they require
//! infrastructure (provider, shared state) not available in the generic BuilderContext.
//!
//! For working MEV functionality, see the backtest-runner implementation which
//! directly instantiates components with required dependencies.

use eyre::Result;
use std::marker::PhantomData;

// Core component framework imports
use kabu_core_components::{
    BroadcasterBuilder, BuilderContext, EstimatorBuilder, ExecutorBuilder, HealthMonitorBuilderTrait, MarketBuilder, MevComponentsBuilder,
    NetworkBuilder, PoolBuilder, SignerBuilderTrait, StrategyBuilder,
};

#[allow(unused_imports)]
use kabu_core_components::PlaceholderComponent;

// Component implementations
use kabu_broadcast_broadcaster::FlashbotsBroadcastComponent;
use kabu_core_router::SwapRouterComponent;
// Strategy imports removed - using PlaceholderComponent until proper setup is implemented

#[cfg(feature = "defi-health-monitor")]
use kabu_defi_health_monitor::PoolHealthMonitorComponent;

// Type imports
use kabu_evm_db::KabuDB;
use kabu_types_blockchain::KabuDataTypesEthereum;
use kabu_types_market::Market;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Kabu Node providing MEV bot functionality
#[derive(Clone, Default)]
pub struct KabuNode;

impl KabuNode {
    pub fn new() -> Self {
        Self
    }

    /// Get the default MEV components configuration
    #[allow(clippy::type_complexity)]
    pub fn components<State>() -> MevComponentsBuilder<
        State,
        KabuPoolBuilder<State>,
        KabuNetworkBuilder<State>,
        KabuExecutorBuilder<State>,
        KabuStrategyBuilder<State>,
        KabuSignerBuilder<State>,
        KabuMarketBuilder<State>,
        KabuBroadcasterBuilder<State>,
        KabuEstimatorBuilder<State>,
        KabuHealthMonitorBuilder<State>,
    > {
        MevComponentsBuilder::new()
            .pool(KabuPoolBuilder::new())
            .network(KabuNetworkBuilder::new())
            .executor(KabuExecutorBuilder::new())
            .strategy(KabuStrategyBuilder::new())
            .signer(KabuSignerBuilder::new())
            .market(KabuMarketBuilder::new())
            .broadcaster(KabuBroadcasterBuilder::new())
            .estimator(KabuEstimatorBuilder::new())
            .health_monitor(KabuHealthMonitorBuilder::new())
    }
}

// ================================================================================================
// Pool Builder - Handles mempool and transaction pools
// ================================================================================================

#[derive(Clone)]
pub struct KabuPoolBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuPoolBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuPoolBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> PoolBuilder<State> for KabuPoolBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Pool = PlaceholderComponent;

    async fn build_pool(self, _ctx: &BuilderContext<State>) -> Result<Self::Pool> {
        // Note: MempoolComponent requires complex setup with channels and chain params
        Ok(PlaceholderComponent::new("PoolComponent"))
    }
}

// ================================================================================================
// Network Builder - Handles blockchain connections and block processing
// ================================================================================================

#[derive(Clone)]
pub struct KabuNetworkBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuNetworkBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuNetworkBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> NetworkBuilder<State> for KabuNetworkBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Network = PlaceholderComponent;

    async fn build_network(self, _ctx: &BuilderContext<State>) -> Result<Self::Network> {
        // Note: BlockHistoryComponent requires provider setup
        Ok(PlaceholderComponent::new("NetworkComponent"))
    }
}

// ================================================================================================
// Executor Builder - Handles transaction execution and routing
// ================================================================================================

#[derive(Clone)]
pub struct KabuExecutorBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuExecutorBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuExecutorBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> ExecutorBuilder<State> for KabuExecutorBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Executor = SwapRouterComponent<KabuDB, KabuDataTypesEthereum>;

    async fn build_executor(self, ctx: &BuilderContext<State>) -> Result<Self::Executor> {
        let component =
            SwapRouterComponent::new(ctx.channels.signers.clone(), ctx.channels.account_state.clone(), ctx.channels.swap_compose.clone());
        Ok(component)
    }
}

// ================================================================================================
// Strategy Builder - Handles MEV strategies like arbitrage
// ================================================================================================

#[derive(Clone)]
pub struct KabuStrategyBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuStrategyBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuStrategyBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> StrategyBuilder<State> for KabuStrategyBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Strategy = PlaceholderComponent;

    async fn build_strategy(self, _ctx: &BuilderContext<State>) -> Result<Self::Strategy> {
        // IMPORTANT: This is currently a placeholder. The backtest runner uses StateChangeArbComponent
        // which creates internal state_update channels and spawns StateChangeArbSearcherActor.
        // StateChangeArbSearcherActor cannot be used directly as it needs:
        // 1. state_update_rx channel (not in MevComponentChannels)
        // 2. Provider/client for blockchain access
        // 3. Proper market state and mempool setup
        // This is why backtest-runner may show errors like "CANNOT_MERGE_BOTH_SIDES"
        Ok(PlaceholderComponent::new("StrategyComponent"))
    }
}

// ================================================================================================
// Signer Builder - Handles transaction signing
// ================================================================================================

#[derive(Clone)]
pub struct KabuSignerBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuSignerBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuSignerBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> SignerBuilderTrait<State> for KabuSignerBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Signer = PlaceholderComponent;

    async fn build_signer(self, _ctx: &BuilderContext<State>) -> Result<Self::Signer> {
        // Note: SignersComponent requires provider and accounts setup
        Ok(PlaceholderComponent::new("SignerComponent"))
    }
}

// ================================================================================================
// Market Builder - Handles market data, pools, and tokens
// ================================================================================================

#[derive(Clone)]
pub struct KabuMarketBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuMarketBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuMarketBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> MarketBuilder<State> for KabuMarketBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Market = PlaceholderComponent;

    async fn build_market(self, _ctx: &BuilderContext<State>) -> Result<Self::Market> {
        // Note: ProtocolPoolLoaderComponent requires provider and market setup
        Ok(PlaceholderComponent::new("MarketComponent"))
    }
}

// ================================================================================================
// Broadcaster Builder - Handles flashbots and mempool broadcasting
// ================================================================================================

#[derive(Clone)]
pub struct KabuBroadcasterBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuBroadcasterBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuBroadcasterBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> BroadcasterBuilder<State> for KabuBroadcasterBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Broadcaster = FlashbotsBroadcastComponent;

    async fn build_broadcaster(self, ctx: &BuilderContext<State>) -> Result<Self::Broadcaster> {
        // Create with no signer (will use random) and broadcasting disabled for safety
        let component = FlashbotsBroadcastComponent::new(None, false)?.with_default_relays()?.with_channel(ctx.channels.tx_compose.clone());
        Ok(component)
    }
}

// ================================================================================================
// Estimator Builder - Handles gas and profit estimation
// ================================================================================================

#[derive(Clone)]
pub struct KabuEstimatorBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuEstimatorBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuEstimatorBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> EstimatorBuilder<State> for KabuEstimatorBuilder<State>
where
    State: Send + Sync + 'static,
{
    type Estimator = PlaceholderComponent;

    async fn build_estimator(self, _ctx: &BuilderContext<State>) -> Result<Self::Estimator> {
        // Note: EvmEstimatorComponent requires provider and encoder setup
        Ok(PlaceholderComponent::new("EstimatorComponent"))
    }
}

// ================================================================================================
// Health Monitor Builder - Handles system health monitoring
// ================================================================================================

#[derive(Clone)]
pub struct KabuHealthMonitorBuilder<State> {
    _state: PhantomData<State>,
}

impl<State> Default for KabuHealthMonitorBuilder<State> {
    fn default() -> Self {
        Self::new()
    }
}

impl<State> KabuHealthMonitorBuilder<State> {
    pub fn new() -> Self {
        Self { _state: PhantomData }
    }
}

impl<State> HealthMonitorBuilderTrait<State> for KabuHealthMonitorBuilder<State>
where
    State: Send + Sync + 'static,
{
    #[cfg(feature = "defi-health-monitor")]
    type HealthMonitor = PoolHealthMonitorComponent;

    #[cfg(not(feature = "defi-health-monitor"))]
    type HealthMonitor = PlaceholderComponent;

    async fn build_health_monitor(self, ctx: &BuilderContext<State>) -> Result<Self::HealthMonitor> {
        #[cfg(feature = "defi-health-monitor")]
        {
            // Wire up PoolHealthMonitorComponent with proper channels following Reth pattern
            let market = Arc::new(RwLock::new(Market::default()));
            let health_monitor = PoolHealthMonitorComponent::new().with_channels(
                market,
                ctx.channels.health_events.clone(),
                None, // No influxdb for demo
            );

            tracing::info!("PoolHealthMonitorComponent built with channels");
            tracing::info!("Connected to health_events channel for monitoring pool health");

            Ok(health_monitor)
        }

        #[cfg(not(feature = "defi-health-monitor"))]
        {
            Ok(PlaceholderComponent::new("HealthMonitorComponent"))
        }
    }
}
