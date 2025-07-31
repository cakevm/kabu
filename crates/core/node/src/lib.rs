//! Kabu MEV Bot Node Implementation

use alloy_network::Ethereum;
use alloy_primitives::{hex, Address};
use alloy_provider::Provider;
use eyre::Result;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::RwLock;

// Core component framework imports
use kabu_core_components::{
    BroadcasterBuilder, BuilderContext, Component, EstimatorBuilder, ExecutorBuilder, HealthMonitorBuilderTrait, MarketBuilder,
    MergerBuilder, MevComponentChannels, MevComponentsBuilder, MonitoringBuilder, NetworkBuilder, PlaceholderComponent, PoolBuilder,
    SignerBuilderTrait, StrategyBuilder, WebServerBuilder,
};

// Component implementations
use kabu_broadcast_accounts::{AccountMonitorComponent, InitializeSignersOneShotBlockingComponent, SignersComponent};
use kabu_broadcast_broadcaster::FlashbotsBroadcastComponent;
use kabu_core_block_history::BlockHistoryComponent;
use kabu_core_router::SwapRouterComponent;
use kabu_core_topology::{BroadcasterConfig, TopologyConfig};
use kabu_defi_market::{HistoryPoolLoaderComponent, ProtocolPoolLoaderComponent};
use kabu_defi_pools::{PoolLoadersBuilder, PoolsLoadingConfig};
use kabu_defi_preloader::MarketStatePreloadedOneShotComponent;
use kabu_defi_price::PriceComponent;
use kabu_execution_estimator::EvmEstimatorComponent;
use kabu_execution_multicaller::MulticallerSwapEncoder;
use kabu_metrics::InfluxDbWriterComponent;
use kabu_node_config::NodeBlockComponentConfig;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_node_json_rpc::BlockProcessingComponent;
use kabu_rpc_handler::WebServerComponent;
use kabu_storage_db::DbPool;
use kabu_strategy_backrun::{BackrunConfig, StateChangeArbComponent};
use kabu_strategy_merger::{ArbSwapPathMergerComponent, DiffPathMergerComponent, SamePathMergerComponent};

#[cfg(feature = "defi-health-monitor")]
use kabu_defi_health_monitor::PoolHealthMonitorComponent;

// Type imports
use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_evm_db::{DatabaseKabuExt, KabuDBError};
use kabu_types_blockchain::{ChainParameters, KabuDataTypesEthereum, Mempool};
use kabu_types_entities::{BlockHistory, BlockHistoryState, LatestBlock};
use kabu_types_market::{Market, MarketState};
use reth::revm::{Database, DatabaseCommit, DatabaseRef};

/// Extended build context for Kabu components with all necessary resources
#[derive(Clone)]
pub struct KabuBuildContext<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    /// Provider for blockchain access
    pub provider: P,
    /// Blockchain state
    pub blockchain: Blockchain,
    /// Blockchain state with database
    pub blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
    /// MEV component channels
    pub channels: MevComponentChannels<DB>,
    /// Topology configuration
    pub topology_config: TopologyConfig,
    /// Backrun configuration
    pub backrun_config: BackrunConfig,
    /// Multicaller address
    pub multicaller_address: Address,
    /// Multicaller encoder
    pub swap_encoder: MulticallerSwapEncoder,
    /// Database pool
    pub db_pool: DbPool,
    /// Pool loading configuration
    pub pools_config: PoolsLoadingConfig,
    /// Whether running as ExEx
    pub is_exex: bool,
    /// Shared market state
    pub market: Arc<RwLock<Market>>,
    /// Shared market state with DB
    pub market_state: Arc<RwLock<MarketState<DB>>>,
    /// Shared mempool
    pub mempool: Arc<RwLock<Mempool<KabuDataTypesEthereum>>>,
    /// Shared block history
    pub block_history: Arc<RwLock<BlockHistory<DB, KabuDataTypesEthereum>>>,
    /// Shared latest block
    pub latest_block: Arc<RwLock<LatestBlock<KabuDataTypesEthereum>>>,
    // Note: Block channels and influxdb channel are accessed via blockchain methods:
    // - blockchain.new_block_headers_channel()
    // - blockchain.new_block_with_tx_channel()
    // - blockchain.new_block_state_update_channel()
    // - blockchain.new_block_logs_channel()
    // - blockchain.influxdb_write_channel()
}

impl<P, DB> KabuBuildContext<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    /// Create a new KabuBuildContext with defaults
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: P,
        blockchain: Blockchain,
        blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
        topology_config: TopologyConfig,
        backrun_config: BackrunConfig,
        multicaller_address: Address,
        db_pool: DbPool,
        is_exex: bool,
    ) -> Self {
        let swap_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);
        let mev_channels = MevComponentChannels::default();

        // Get shared state from blockchain
        let market = blockchain.market();
        let market_state = blockchain_state.market_state_commit();
        let mempool = blockchain.mempool();
        let block_history = Arc::new(RwLock::new(BlockHistory::new(10)));
        let latest_block = blockchain.latest_block();

        // Default pools config
        let pools_config = PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default())
            .enable(kabu_types_market::PoolClass::UniswapV2)
            .enable(kabu_types_market::PoolClass::UniswapV3);

        Self {
            provider,
            blockchain,
            blockchain_state,
            channels: mev_channels,
            topology_config,
            backrun_config,
            multicaller_address,
            swap_encoder,
            db_pool,
            pools_config,
            is_exex,
            market,
            market_state,
            mempool,
            block_history,
            latest_block,
        }
    }

    /// Create a builder for customizing the context
    #[allow(clippy::too_many_arguments)]
    pub fn builder(
        provider: P,
        blockchain: Blockchain,
        blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
        topology_config: TopologyConfig,
        backrun_config: BackrunConfig,
        multicaller_address: Address,
        db_pool: DbPool,
        is_exex: bool,
    ) -> KabuBuildContextBuilder<P, DB> {
        KabuBuildContextBuilder::new(
            provider,
            blockchain,
            blockchain_state,
            topology_config,
            backrun_config,
            multicaller_address,
            db_pool,
            is_exex,
        )
    }
}

/// Builder for KabuBuildContext
pub struct KabuBuildContextBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    provider: P,
    blockchain: Blockchain,
    blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
    channels: MevComponentChannels<DB>,
    topology_config: TopologyConfig,
    backrun_config: BackrunConfig,
    multicaller_address: Address,
    swap_encoder: MulticallerSwapEncoder,
    db_pool: DbPool,
    pools_config: PoolsLoadingConfig,
    is_exex: bool,
    market: Arc<RwLock<Market>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    mempool: Arc<RwLock<Mempool<KabuDataTypesEthereum>>>,
    block_history: Arc<RwLock<BlockHistory<DB, KabuDataTypesEthereum>>>,
    latest_block: Arc<RwLock<LatestBlock<KabuDataTypesEthereum>>>,
}

impl<P, DB> KabuBuildContextBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: P,
        blockchain: Blockchain,
        blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
        topology_config: TopologyConfig,
        backrun_config: BackrunConfig,
        multicaller_address: Address,
        db_pool: DbPool,
        is_exex: bool,
    ) -> Self {
        let swap_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);
        let mev_channels = MevComponentChannels::default();

        // Create default channels
        // Get shared state from blockchain
        let market = blockchain.market();
        let market_state = blockchain_state.market_state_commit();
        let mempool = blockchain.mempool();
        let block_history = Arc::new(RwLock::new(BlockHistory::new(10)));
        let latest_block = blockchain.latest_block();

        // Default pools config
        let pools_config = PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default())
            .enable(kabu_types_market::PoolClass::UniswapV2)
            .enable(kabu_types_market::PoolClass::UniswapV3);

        Self {
            provider,
            blockchain,
            blockchain_state,
            channels: mev_channels,
            topology_config,
            backrun_config,
            multicaller_address,
            swap_encoder,
            db_pool,
            pools_config,
            is_exex,
            market,
            market_state,
            mempool,
            block_history,
            latest_block,
        }
    }

    pub fn with_channels(mut self, channels: MevComponentChannels<DB>) -> Self {
        self.channels = channels;
        self
    }

    pub fn with_pools_config(mut self, pools_config: PoolsLoadingConfig) -> Self {
        self.pools_config = pools_config;
        self
    }

    pub fn with_market(mut self, market: Arc<RwLock<Market>>) -> Self {
        self.market = market;
        self
    }

    pub fn with_market_state(mut self, market_state: Arc<RwLock<MarketState<DB>>>) -> Self {
        self.market_state = market_state;
        self
    }

    pub fn with_mempool(mut self, mempool: Arc<RwLock<Mempool<KabuDataTypesEthereum>>>) -> Self {
        self.mempool = mempool;
        self
    }

    pub fn with_block_history(mut self, block_history: Arc<RwLock<BlockHistory<DB, KabuDataTypesEthereum>>>) -> Self {
        self.block_history = block_history;
        self
    }

    pub fn with_latest_block(mut self, latest_block: Arc<RwLock<LatestBlock<KabuDataTypesEthereum>>>) -> Self {
        self.latest_block = latest_block;
        self
    }

    pub fn with_swap_encoder(mut self, encoder: MulticallerSwapEncoder) -> Self {
        self.swap_encoder = encoder;
        self
    }

    pub fn build(self) -> KabuBuildContext<P, DB> {
        KabuBuildContext {
            provider: self.provider,
            blockchain: self.blockchain,
            blockchain_state: self.blockchain_state,
            channels: self.channels,
            topology_config: self.topology_config,
            backrun_config: self.backrun_config,
            multicaller_address: self.multicaller_address,
            swap_encoder: self.swap_encoder,
            db_pool: self.db_pool,
            pools_config: self.pools_config,
            is_exex: self.is_exex,
            market: self.market,
            market_state: self.market_state,
            mempool: self.mempool,
            block_history: self.block_history,
            latest_block: self.latest_block,
        }
    }
}

/// Kabu Node providing MEV bot functionality
#[derive(Clone, Default)]
pub struct KabuNode;

/// Trait to extract KabuBuildContext from generic BuilderContext
pub trait AsKabuContext<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    fn as_kabu_context(&self) -> Result<&KabuBuildContext<P, DB>>;
}

impl<P, DB> AsKabuContext<P, DB> for BuilderContext<KabuBuildContext<P, DB>>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    fn as_kabu_context(&self) -> Result<&KabuBuildContext<P, DB>> {
        Ok(&self.state)
    }
}

impl KabuNode {
    pub fn new() -> Self {
        Self
    }

    /// Get the default MEV components configuration for KabuBuildContext
    #[allow(clippy::type_complexity)]
    pub fn components<P, DB>() -> MevComponentsBuilder<
        KabuBuildContext<P, DB>,
        KabuPoolBuilder<P, DB>,
        KabuNetworkBuilder<P, DB>,
        KabuExecutorBuilder<P, DB>,
        KabuStrategyBuilder<P, DB>,
        KabuSignerBuilder<P, DB>,
        KabuMarketBuilder<P, DB>,
        KabuBroadcasterBuilder<P, DB>,
        KabuEstimatorBuilder<P, DB>,
        KabuHealthMonitorBuilder<P, DB>,
    >
    where
        P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
        DB: DatabaseRef<Error = KabuDBError>
            + Database<Error = KabuDBError>
            + DatabaseCommit
            + DatabaseKabuExt
            + BlockHistoryState<KabuDataTypesEthereum>
            + Send
            + Sync
            + Clone
            + Default
            + 'static,
    {
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
pub struct KabuPoolBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuPoolBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuPoolBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> PoolBuilder<KabuBuildContext<P, DB>> for KabuPoolBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Pool = PlaceholderComponent;

    async fn build_pool(self, _ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Pool> {
        // Mempool is already created and managed in KabuBuildContext
        // Could return a MempoolComponent here if needed
        Ok(PlaceholderComponent::new("PoolComponent"))
    }
}

// ================================================================================================
// Network Builder - Handles blockchain connections and block processing
// ================================================================================================

#[derive(Clone)]
pub struct KabuNetworkBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuNetworkBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuNetworkBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

/// Composite network component for both block processing and history
pub struct CompositeNetworkComponent {
    components: Vec<Box<dyn Component>>,
}

impl Clone for CompositeNetworkComponent {
    fn clone(&self) -> Self {
        // We cannot clone the components, so return empty
        Self { components: Vec::new() }
    }
}

impl CompositeNetworkComponent {
    pub fn new(components: Vec<Box<dyn Component>>) -> Self {
        Self { components }
    }
}

impl Component for CompositeNetworkComponent {
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        for component in self.components {
            component.spawn_boxed(executor.clone())?;
        }
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: reth_tasks::TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "CompositeNetworkComponent"
    }
}

impl<P, DB> NetworkBuilder<KabuBuildContext<P, DB>> for KabuNetworkBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Network = CompositeNetworkComponent;

    async fn build_network(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Network> {
        let kabu_ctx = ctx.as_kabu_context()?;
        let mut components: Vec<Box<dyn Component>> = Vec::new();

        // Block processing component
        let block_processing = BlockProcessingComponent::new(kabu_ctx.provider.clone(), NodeBlockComponentConfig::all_enabled())
            .with_channels(
                Some(kabu_ctx.blockchain.new_block_headers_channel()),
                Some(kabu_ctx.blockchain.new_block_with_tx_channel()),
                Some(kabu_ctx.blockchain.new_block_logs_channel()),
                Some(kabu_ctx.blockchain.new_block_state_update_channel()),
            );
        components.push(Box::new(block_processing));

        // Block history component
        let block_history = BlockHistoryComponent::new(kabu_ctx.provider.clone()).with_channels(
            ChainParameters::ethereum(),
            kabu_ctx.latest_block.clone(),
            kabu_ctx.market_state.clone(),
            kabu_ctx.block_history.clone(),
            kabu_ctx.blockchain.new_block_headers_channel(),
            kabu_ctx.blockchain.new_block_with_tx_channel(),
            kabu_ctx.blockchain.new_block_logs_channel(),
            kabu_ctx.blockchain.new_block_state_update_channel(),
            kabu_ctx.channels.market_events.clone(),
        );
        components.push(Box::new(block_history));

        Ok(CompositeNetworkComponent::new(components))
    }
}

// ================================================================================================
// Executor Builder - Handles transaction execution and routing
// ================================================================================================

#[derive(Clone)]
pub struct KabuExecutorBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuExecutorBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuExecutorBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> ExecutorBuilder<KabuBuildContext<P, DB>> for KabuExecutorBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Executor = SwapRouterComponent<DB, KabuDataTypesEthereum>;

    async fn build_executor(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Executor> {
        let kabu_ctx = ctx.as_kabu_context()?;

        let component = SwapRouterComponent::new(
            kabu_ctx.channels.signers.clone(),
            kabu_ctx.channels.account_state.clone(),
            kabu_ctx.channels.swap_compose.clone(),
        );
        Ok(component)
    }
}

// ================================================================================================
// Strategy Builder - Handles MEV strategies like arbitrage
// ================================================================================================

#[derive(Clone)]
pub struct KabuStrategyBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuStrategyBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuStrategyBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> StrategyBuilder<KabuBuildContext<P, DB>> for KabuStrategyBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Strategy = StateChangeArbComponent<P, Ethereum, DB, KabuDataTypesEthereum>;

    async fn build_strategy(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Strategy> {
        let kabu_ctx = ctx.as_kabu_context()?;

        let mut component = StateChangeArbComponent::<_, _, DB, _>::new(
            kabu_ctx.provider.clone(),
            true,              // use_blocks
            !kabu_ctx.is_exex, // use_mempool (only if not exex)
            kabu_ctx.backrun_config.clone(),
        )
        .with_market(kabu_ctx.market.clone())
        .with_mempool(kabu_ctx.mempool.clone())
        .with_latest_block(kabu_ctx.latest_block.clone())
        .with_market_state(kabu_ctx.market_state.clone())
        .with_block_history(kabu_ctx.block_history.clone())
        .with_mempool_events_channel(kabu_ctx.channels.mempool_events.clone())
        .with_market_events_channel(kabu_ctx.channels.market_events.clone())
        .with_swap_compose_channel(kabu_ctx.channels.swap_compose.clone())
        .with_pool_health_monitor_channel(kabu_ctx.channels.health_events.clone());

        if let Some(channel) = kabu_ctx.blockchain.influxdb_write_channel() {
            component = component.with_influxdb_channel(channel);
        }

        Ok(component)
    }
}

// ================================================================================================
// Signer Builder - Handles transaction signing
// ================================================================================================

#[derive(Clone)]
pub struct KabuSignerBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuSignerBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuSignerBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> SignerBuilderTrait<KabuBuildContext<P, DB>> for KabuSignerBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Signer = SignersComponent<P, Ethereum, DB, KabuDataTypesEthereum>;

    async fn build_signer(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Signer> {
        let kabu_ctx = ctx.as_kabu_context()?;

        let component = SignersComponent::new(
            kabu_ctx.provider.clone(),
            kabu_ctx.channels.signers.clone(),
            kabu_ctx.channels.account_state.clone(),
            120, // gas_price_buffer
        )
        .with_channels(kabu_ctx.channels.swap_compose.clone(), kabu_ctx.channels.swap_compose.clone());

        Ok(component)
    }
}

// ================================================================================================
// Market Builder - Handles market data, pools, and tokens
// ================================================================================================

#[derive(Clone)]
pub struct KabuMarketBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuMarketBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuMarketBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

/// Composite market component that manages multiple market-related sub-components
pub struct CompositeMarketComponent {
    components: Vec<Box<dyn Component>>,
}

impl Clone for CompositeMarketComponent {
    fn clone(&self) -> Self {
        // We cannot clone the components, so return empty
        Self { components: Vec::new() }
    }
}

impl CompositeMarketComponent {
    pub fn new(components: Vec<Box<dyn Component>>) -> Self {
        Self { components }
    }
}

impl Component for CompositeMarketComponent {
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        for component in self.components {
            component.spawn_boxed(executor.clone())?;
        }
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: reth_tasks::TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "CompositeMarketComponent"
    }
}

impl<P, DB> MarketBuilder<KabuBuildContext<P, DB>> for KabuMarketBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Market = CompositeMarketComponent;

    async fn build_market(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Market> {
        let kabu_ctx = ctx.as_kabu_context()?;
        let mut components: Vec<Box<dyn Component>> = Vec::new();

        // Initialize signers (one-shot)
        if let Ok(key) = std::env::var("DATA") {
            let private_key_encrypted = hex::decode(key)?;
            let initializer = InitializeSignersOneShotBlockingComponent::new(Some(private_key_encrypted))
                .with_signers(kabu_ctx.channels.signers.clone())
                .with_monitor(kabu_ctx.channels.account_state.clone());
            components.push(Box::new(initializer));
        }

        // Market state preloader (one-shot)
        let market_state_preload = MarketStatePreloadedOneShotComponent::new(kabu_ctx.provider.clone())
            .with_copied_account(kabu_ctx.swap_encoder.get_contract_address())
            .with_signers(kabu_ctx.channels.signers.clone())
            .with_market_state(kabu_ctx.market_state.clone());
        components.push(Box::new(market_state_preload));

        // Price component (one-shot)
        let price_component = PriceComponent::new(kabu_ctx.provider.clone()).only_once().with_market(kabu_ctx.market.clone());
        components.push(Box::new(price_component));

        // Account monitor
        let account_monitor = AccountMonitorComponent::new(
            kabu_ctx.provider.clone(),
            kabu_ctx.channels.account_state.clone(),
            kabu_ctx.channels.signers.clone(),
            std::time::Duration::from_secs(1),
        );
        components.push(Box::new(account_monitor));

        // Pool loaders
        let pool_loaders = Arc::new(PoolLoadersBuilder::<_, _, KabuDataTypesEthereum>::default_pool_loaders(
            kabu_ctx.provider.clone(),
            kabu_ctx.pools_config.clone(),
        ));

        // Protocol pool loader
        let protocol_loader = ProtocolPoolLoaderComponent::new(kabu_ctx.provider.clone(), pool_loaders.clone());
        components.push(Box::new(protocol_loader));

        // History pool loader
        let history_loader = HistoryPoolLoaderComponent::new(
            kabu_ctx.provider.clone(),
            pool_loaders,
            0,    // start_block
            1000, // block_batch_size
            10,   // max_batches
        );
        components.push(Box::new(history_loader));

        Ok(CompositeMarketComponent::new(components))
    }
}

// ================================================================================================
// Broadcaster Builder - Handles flashbots and mempool broadcasting
// ================================================================================================

#[derive(Clone)]
pub struct KabuBroadcasterBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuBroadcasterBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuBroadcasterBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> BroadcasterBuilder<KabuBuildContext<P, DB>> for KabuBroadcasterBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Broadcaster = FlashbotsBroadcastComponent;

    async fn build_broadcaster(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Broadcaster> {
        let kabu_ctx = ctx.as_kabu_context()?;

        // Get flashbots relays from config
        let relays = kabu_ctx
            .topology_config
            .actors
            .broadcaster
            .as_ref()
            .and_then(|b| b.get("mainnet"))
            .map(|b| match b {
                BroadcasterConfig::Flashbots(f) => f.relays(),
            })
            .unwrap_or_default();

        // Create with no signer (will use random) and broadcasting enabled if we have relays
        let component =
            FlashbotsBroadcastComponent::new(None, !relays.is_empty())?.with_relays(relays)?.with_channel(ctx.channels.tx_compose.clone());
        Ok(component)
    }
}

// ================================================================================================
// Estimator Builder - Handles gas and profit estimation
// ================================================================================================

#[derive(Clone)]
pub struct KabuEstimatorBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuEstimatorBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuEstimatorBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> EstimatorBuilder<KabuBuildContext<P, DB>> for KabuEstimatorBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Estimator = EvmEstimatorComponent<P, Ethereum, MulticallerSwapEncoder, DB>;

    async fn build_estimator(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Estimator> {
        let kabu_ctx = ctx.as_kabu_context()?;

        let component =
            EvmEstimatorComponent::<_, Ethereum, _, DB>::new_with_provider(kabu_ctx.swap_encoder.clone(), Some(kabu_ctx.provider.clone()))
                .with_swap_compose_channel(kabu_ctx.channels.swap_compose.clone());

        Ok(component)
    }
}

// ================================================================================================
// Health Monitor Builder - Handles system health monitoring
// ================================================================================================

#[derive(Clone)]
pub struct KabuHealthMonitorBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuHealthMonitorBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuHealthMonitorBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> HealthMonitorBuilderTrait<KabuBuildContext<P, DB>> for KabuHealthMonitorBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    #[cfg(feature = "defi-health-monitor")]
    type HealthMonitor = PoolHealthMonitorComponent;

    #[cfg(not(feature = "defi-health-monitor"))]
    type HealthMonitor = PlaceholderComponent;

    async fn build_health_monitor(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::HealthMonitor> {
        #[cfg(feature = "defi-health-monitor")]
        {
            let kabu_ctx = ctx.as_kabu_context()?;

            let health_monitor = PoolHealthMonitorComponent::new().with_channels(
                kabu_ctx.market.clone(),
                ctx.channels.health_events.clone(),
                kabu_ctx.blockchain.influxdb_write_channel(),
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

// ================================================================================================
// Merger Builder - Handles merger components for combining arbitrage paths
// ================================================================================================

#[derive(Clone)]
pub struct KabuMergerBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuMergerBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuMergerBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

/// Composite merger component that manages all merger sub-components
pub struct CompositeMergerComponent {
    components: Vec<Box<dyn Component>>,
}

impl Clone for CompositeMergerComponent {
    fn clone(&self) -> Self {
        // We cannot clone the components, so return empty
        Self { components: Vec::new() }
    }
}

impl CompositeMergerComponent {
    pub fn new(components: Vec<Box<dyn Component>>) -> Self {
        Self { components }
    }
}

impl Component for CompositeMergerComponent {
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        for component in self.components {
            component.spawn_boxed(executor.clone())?;
        }
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: reth_tasks::TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "CompositeMergerComponent"
    }
}

impl<P, DB> MergerBuilder<KabuBuildContext<P, DB>> for KabuMergerBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Merger = CompositeMergerComponent;

    async fn build_merger(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Merger> {
        let kabu_ctx = ctx.as_kabu_context()?;
        let mut components: Vec<Box<dyn Component>> = Vec::new();

        // Get the swap compose channel from the channels
        let swap_compose_channel = kabu_ctx.channels.swap_compose.clone();

        // Swap path merger
        let swap_path_merger = ArbSwapPathMergerComponent::<DB>::new(kabu_ctx.multicaller_address)
            .with_latest_block(kabu_ctx.latest_block.clone())
            .with_market_events_channel(kabu_ctx.channels.market_events.clone())
            .with_compose_channel(swap_compose_channel.clone());
        components.push(Box::new(swap_path_merger));

        // Same path merger
        let same_path_merger = SamePathMergerComponent::<_, _, DB>::new(kabu_ctx.provider.clone())
            .with_market_state(kabu_ctx.market_state.clone())
            .with_latest_block(kabu_ctx.latest_block.clone())
            .with_market_events_channel(kabu_ctx.channels.market_events.clone())
            .with_compose_channel(swap_compose_channel.clone());
        components.push(Box::new(same_path_merger));

        // Diff path merger
        let diff_path_merger = DiffPathMergerComponent::<DB>::new()
            .with_market_events_channel(kabu_ctx.channels.market_events.clone())
            .with_compose_channel(swap_compose_channel);
        components.push(Box::new(diff_path_merger));

        Ok(CompositeMergerComponent::new(components))
    }
}

// ================================================================================================
// Web Server Builder - Handles web server component
// ================================================================================================

#[derive(Clone)]
pub struct KabuWebServerBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuWebServerBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuWebServerBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> WebServerBuilder<KabuBuildContext<P, DB>> for KabuWebServerBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type WebServer = WebServerComponent<(), DB>;

    async fn build_web_server(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::WebServer> {
        let kabu_ctx = ctx.as_kabu_context()?;

        let webserver_host = kabu_ctx.topology_config.webserver.clone().unwrap_or_default().host;

        let web_server = WebServerComponent::<(), DB>::new(
            webserver_host,
            axum::Router::new(), // Empty router for now
            kabu_ctx.db_pool.clone(),
            tokio_util::sync::CancellationToken::new(),
        )
        .on_bc(&kabu_ctx.blockchain, &kabu_ctx.blockchain_state);

        Ok(web_server)
    }
}

// ================================================================================================
// Monitoring Builder - Handles InfluxDB and other monitoring components
// ================================================================================================

/// Composite monitoring component that manages monitoring sub-components
pub struct CompositeMonitoringComponent {
    components: Vec<Box<dyn Component>>,
}

impl Clone for CompositeMonitoringComponent {
    fn clone(&self) -> Self {
        // We cannot clone the components, so return empty
        Self { components: Vec::new() }
    }
}

impl CompositeMonitoringComponent {
    pub fn new(components: Vec<Box<dyn Component>>) -> Self {
        Self { components }
    }
}

impl Component for CompositeMonitoringComponent {
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        for component in self.components {
            component.spawn_boxed(executor.clone())?;
        }
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: reth_tasks::TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "CompositeMonitoringComponent"
    }
}

#[derive(Clone)]
pub struct KabuMonitoringBuilder<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> Default for KabuMonitoringBuilder<P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, DB> KabuMonitoringBuilder<P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<P, DB> MonitoringBuilder<KabuBuildContext<P, DB>> for KabuMonitoringBuilder<P, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    type Monitoring = CompositeMonitoringComponent;

    async fn build_monitoring(self, ctx: &BuilderContext<KabuBuildContext<P, DB>>) -> Result<Self::Monitoring> {
        let kabu_ctx = ctx.as_kabu_context()?;
        let mut components: Vec<Box<dyn Component>> = Vec::new();

        if let Some(influxdb_config) = &kabu_ctx.topology_config.influxdb {
            let influxdb_writer =
                InfluxDbWriterComponent::new(influxdb_config.url.clone(), influxdb_config.database.clone(), influxdb_config.tags.clone())
                    .with_channel(kabu_ctx.blockchain.influxdb_write_channel());

            components.push(Box::new(influxdb_writer));
        }

        Ok(CompositeMonitoringComponent::new(components))
    }
}
