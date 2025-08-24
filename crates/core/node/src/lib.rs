//! Kabu MEV Bot Node Implementation

use alloy_network::Ethereum;
use alloy_primitives::Address;
use alloy_provider::Provider;
use eyre::Result;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::RwLock;

use reth_chain_state::CanonStateSubscriptions;
use reth_ethereum_primitives::EthPrimitives;

// Core component framework imports
use kabu_core_components::{
    BroadcasterBuilder, BuilderContext, Component, EstimatorBuilder, ExecutorBuilder, HealthMonitorBuilderTrait, KabuComponentsSet,
    KabuNode as KabuNodeTrait, KabuNodeComponentsBuilder, KabuNodeTypes, MarketBuilder, MergerBuilder, MevComponentChannels,
    MevComponentsBuilder, MonitoringBuilder, NetworkBuilder, PlaceholderComponent, PoolBuilder, SignerBuilderTrait, StrategyBuilder,
    WebServerBuilder,
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
use kabu_storage_db::DbPool;
use kabu_strategy_backrun::{BackrunConfig, StateChangeArbComponent};
use kabu_strategy_merger::{ArbSwapPathMergerComponent, DiffPathMergerComponent, SamePathMergerComponent};

#[cfg(feature = "defi-health-monitor")]
use kabu_defi_health_monitor::PoolHealthMonitorComponent;

// Type imports
use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_evm_db::{DatabaseKabuExt, KabuDBError};
use kabu_types_blockchain::{ChainParameters, KabuDataTypesEthereum, Mempool};
use kabu_types_entities::{BlockHistory, BlockHistoryState};
use kabu_types_market::{Market, MarketState};
use reth::revm::{Database, DatabaseCommit, DatabaseRef};
use reth_evm::ConfigureEvm;
use reth_node_types::NodeTypesWithDB;

/// Extended build context for Kabu components with all necessary resources
#[derive(Clone)]
pub struct KabuBuildContext<N, R, P, DB, Evm>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: reth_evm::ConfigureEvm + 'static,
{
    /// Reth provider for node access
    pub reth_provider: R,
    /// Provider for blockchain access
    pub provider: P,
    /// EVM configuration
    pub evm_config: Evm,
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
    /// Database pool (optional - only needed for web server)
    pub db_pool: Option<DbPool>,
    /// Pool loading configuration
    pub pools_config: PoolsLoadingConfig,
    /// Whether running as ExEx
    pub is_exex: bool,
    /// Whether to spawn web server
    pub enable_web_server: bool,
    /// Shared market state
    pub market: Arc<RwLock<Market>>,
    /// Shared market state with DB
    pub market_state: Arc<RwLock<MarketState<DB>>>,
    /// Shared mempool
    pub mempool: Arc<RwLock<Mempool<KabuDataTypesEthereum>>>,
    /// Shared block history
    pub block_history: Arc<RwLock<BlockHistory<DB, KabuDataTypesEthereum>>>,
    _phantom: std::marker::PhantomData<N>,
}

impl<N, R, P, DB, Evm> KabuBuildContext<N, R, P, DB, Evm>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    /// Create a new KabuBuildContext with defaults
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reth_provider: R,
        provider: P,
        evm_config: Evm,
        blockchain: Blockchain,
        blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
        topology_config: TopologyConfig,
        backrun_config: BackrunConfig,
        multicaller_address: Address,
        db_pool: Option<DbPool>,
        is_exex: bool,
    ) -> Self {
        let swap_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);
        let mev_channels = MevComponentChannels::default();

        // Get shared state from blockchain
        let market = blockchain.market();
        let market_state = blockchain_state.market_state_commit();
        let mempool = blockchain.mempool();
        let block_history = Arc::new(RwLock::new(BlockHistory::new(10)));

        // Default pools config
        let pools_config = PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default())
            .enable(kabu_types_market::PoolClass::UniswapV2)
            .enable(kabu_types_market::PoolClass::UniswapV3);

        Self {
            reth_provider,
            provider,
            evm_config,
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
            enable_web_server: true, // Default to enabled
            market,
            market_state,
            mempool,
            block_history,
            _phantom: PhantomData,
        }
    }

    /// Create a builder for customizing the context
    #[allow(clippy::too_many_arguments)]
    pub fn builder(
        reth_provider: R,
        provider: P,
        evm_config: Evm,
        blockchain: Blockchain,
        blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
        topology_config: TopologyConfig,
        backrun_config: BackrunConfig,
        multicaller_address: Address,
        db_pool: Option<DbPool>,
        is_exex: bool,
    ) -> KabuBuildContextBuilder<N, R, P, DB, Evm> {
        KabuBuildContextBuilder::new(
            reth_provider,
            provider,
            evm_config,
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
pub struct KabuBuildContextBuilder<N, R, P, DB, Evm>
where
    N: reth_node_types::NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    reth_provider: R,
    provider: P,
    evm_config: Evm,
    blockchain: Blockchain,
    blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
    channels: MevComponentChannels<DB>,
    topology_config: TopologyConfig,
    backrun_config: BackrunConfig,
    multicaller_address: Address,
    swap_encoder: MulticallerSwapEncoder,
    db_pool: Option<DbPool>,
    pools_config: PoolsLoadingConfig,
    is_exex: bool,
    enable_web_server: bool,
    market: Arc<RwLock<Market>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    mempool: Arc<RwLock<Mempool<KabuDataTypesEthereum>>>,
    block_history: Arc<RwLock<BlockHistory<DB, KabuDataTypesEthereum>>>,
    _phantom: PhantomData<(N, Evm)>,
}

impl<N, R, P, DB, Evm> KabuBuildContextBuilder<N, R, P, DB, Evm>
where
    N: reth_node_types::NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reth_provider: R,
        provider: P,
        evm_config: Evm,
        blockchain: Blockchain,
        blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
        topology_config: TopologyConfig,
        backrun_config: BackrunConfig,
        multicaller_address: Address,
        db_pool: Option<DbPool>,
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

        // Default pools config
        let pools_config = PoolsLoadingConfig::disable_all(PoolsLoadingConfig::default())
            .enable(kabu_types_market::PoolClass::UniswapV2)
            .enable(kabu_types_market::PoolClass::UniswapV3);

        Self {
            reth_provider,
            provider,
            evm_config,
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
            enable_web_server: true, // Default to enabled
            market,
            market_state,
            mempool,
            block_history,
            _phantom: PhantomData,
        }
    }

    pub fn with_enable_web_server(mut self, enable: bool) -> Self {
        self.enable_web_server = enable;
        self
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

    pub fn with_swap_encoder(mut self, encoder: MulticallerSwapEncoder) -> Self {
        self.swap_encoder = encoder;
        self
    }

    pub fn build(self) -> KabuBuildContext<N, R, P, DB, Evm> {
        KabuBuildContext {
            reth_provider: self.reth_provider,
            provider: self.provider,
            evm_config: self.evm_config,
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
            enable_web_server: self.enable_web_server,
            market: self.market,
            market_state: self.market_state,
            mempool: self.mempool,
            block_history: self.block_history,
            _phantom: PhantomData,
        }
    }
}

/// Kabu Node providing MEV bot functionality
#[derive(Clone, Default)]
pub struct KabuNode;

/// Kabu Ethereum node types configuration
#[derive(Clone)]
pub struct KabuEthereumNode<N, R, P, DB, Evm> {
    _phantom: PhantomData<(N, R, P, DB, Evm)>,
}

impl<N, R, P, DB, Evm> Default for KabuEthereumNode<N, R, P, DB, Evm> {
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> KabuNodeTypes for KabuEthereumNode<N, R, P, DB, Evm>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type State = KabuBuildContext<N, R, P, DB, Evm>;
}

/// Trait to extract KabuBuildContext from generic BuilderContext
pub trait AsKabuContext<N, R, P, DB, Evm>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    fn as_kabu_context(&self) -> Result<&KabuBuildContext<N, R, P, DB, Evm>>;
}

impl<N, R, P, DB, Evm> AsKabuContext<N, R, P, DB, Evm> for BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    fn as_kabu_context(&self) -> Result<&KabuBuildContext<N, R, P, DB, Evm>> {
        Ok(&self.state)
    }
}

impl KabuNode {
    pub fn new() -> Self {
        Self
    }

    /// Get the default MEV components configuration for KabuBuildContext
    #[allow(clippy::type_complexity)]
    pub fn components<N, R, P, DB, Evm>() -> MevComponentsBuilder<
        KabuBuildContext<N, R, P, DB, Evm>,
        KabuPoolBuilder<R, P, DB>,
        KabuNetworkBuilder<R, P, DB>,
        KabuExecutorBuilder<R, P, DB>,
        KabuStrategyBuilder<R, P, DB>,
        KabuSignerBuilder<R, P, DB>,
        KabuMarketBuilder<R, P, DB>,
        KabuBroadcasterBuilder<R, P, DB>,
        KabuEstimatorBuilder<R, P, DB>,
        KabuHealthMonitorBuilder<R, P, DB>,
    >
    where
        N: NodeTypesWithDB,
        R: Send + Sync + Clone + 'static,
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
        Evm: ConfigureEvm + 'static,
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
pub struct KabuPoolBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuPoolBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuPoolBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> PoolBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuPoolBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Pool = PlaceholderComponent;

    async fn build_pool(self, _ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Pool> {
        // Mempool is already created and managed in KabuBuildContext
        // Could return a MempoolComponent here if needed
        Ok(PlaceholderComponent::new("PoolComponent"))
    }
}

// ================================================================================================
// Network Builder - Handles blockchain connections and block processing
// ================================================================================================

#[derive(Clone)]
pub struct KabuNetworkBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuNetworkBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuNetworkBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

/// Composite network component for both block processing and history
pub struct CompositeNetworkComponent<P, DB>
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
    block_processing: BlockProcessingComponent<P, Ethereum, KabuDataTypesEthereum>,
    block_history: BlockHistoryComponent<P, Ethereum, DB, KabuDataTypesEthereum>,
}

impl<P, DB> Clone for CompositeNetworkComponent<P, DB>
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
    fn clone(&self) -> Self {
        Self { block_processing: self.block_processing.clone(), block_history: self.block_history.clone() }
    }
}

impl<P, DB> CompositeNetworkComponent<P, DB>
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
    pub fn new(
        block_processing: BlockProcessingComponent<P, Ethereum, KabuDataTypesEthereum>,
        block_history: BlockHistoryComponent<P, Ethereum, DB, KabuDataTypesEthereum>,
    ) -> Self {
        Self { block_processing, block_history }
    }
}

impl<P, DB> Component for CompositeNetworkComponent<P, DB>
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
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        // Spawn each component directly
        self.block_processing.spawn(executor.clone())?;
        self.block_history.spawn(executor)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "CompositeNetworkComponent"
    }
}

impl<N, R, P, DB, Evm> NetworkBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuNetworkBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Network = CompositeNetworkComponent<P, DB>;

    async fn build_network(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Network> {
        let kabu_ctx = ctx.as_kabu_context()?;

        // Block processing component
        let block_processing = BlockProcessingComponent::<P, Ethereum, KabuDataTypesEthereum>::new(
            kabu_ctx.provider.clone(),
            NodeBlockComponentConfig::all_enabled(),
        )
        .with_channels(
            Some(kabu_ctx.blockchain.new_block_headers_channel()),
            Some(kabu_ctx.blockchain.new_block_with_tx_channel()),
            Some(kabu_ctx.blockchain.new_block_state_update_channel()),
        );

        // Block history component
        let block_history = BlockHistoryComponent::<P, Ethereum, DB, KabuDataTypesEthereum>::new(kabu_ctx.provider.clone()).with_channels(
            ChainParameters::ethereum(),
            kabu_ctx.market_state.clone(),
            kabu_ctx.block_history.clone(),
            kabu_ctx.blockchain.new_block_headers_channel(),
            kabu_ctx.blockchain.new_block_with_tx_channel(),
            kabu_ctx.blockchain.new_block_state_update_channel(),
            kabu_ctx.channels.market_events.clone(),
        );

        Ok(CompositeNetworkComponent::new(block_processing, block_history))
    }
}

// ================================================================================================
// Executor Builder - Handles transaction execution and routing
// ================================================================================================

#[derive(Clone)]
pub struct KabuExecutorBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuExecutorBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuExecutorBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> ExecutorBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuExecutorBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Executor = SwapRouterComponent<DB, KabuDataTypesEthereum>;

    async fn build_executor(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Executor> {
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
pub struct KabuStrategyBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuStrategyBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuStrategyBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> StrategyBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuStrategyBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Strategy = StateChangeArbComponent<P, Ethereum, DB, KabuDataTypesEthereum>;

    async fn build_strategy(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Strategy> {
        let kabu_ctx = ctx.as_kabu_context()?;

        let mut component = StateChangeArbComponent::<_, _, DB, _>::new(
            kabu_ctx.provider.clone(),
            true,              // use_blocks
            !kabu_ctx.is_exex, // use_mempool (only if not exex)
            kabu_ctx.backrun_config.clone(),
        )
        .with_market(kabu_ctx.market.clone())
        .with_mempool(kabu_ctx.mempool.clone())
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
pub struct KabuSignerBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuSignerBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuSignerBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> SignerBuilderTrait<KabuBuildContext<N, R, P, DB, Evm>> for KabuSignerBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Signer = SignersComponent<P, Ethereum, DB, KabuDataTypesEthereum>;

    async fn build_signer(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Signer> {
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
pub struct KabuMarketBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuMarketBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuMarketBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

/// Composite market component that manages multiple market-related sub-components
pub struct CompositeMarketComponent<P, R, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    R: Send + Sync + Clone + 'static,
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
    signer_initializer: Option<InitializeSignersOneShotBlockingComponent<KabuDataTypesEthereum>>,
    market_state_preload: MarketStatePreloadedOneShotComponent<P, Ethereum, DB>,
    price_component: PriceComponent<P, Ethereum>,
    account_monitor: AccountMonitorComponent<P, R, Ethereum, KabuDataTypesEthereum>,
    protocol_loader: ProtocolPoolLoaderComponent<P, P, Ethereum>,
    history_loader: HistoryPoolLoaderComponent<P, P, Ethereum>,
}

impl<P, R, DB> Clone for CompositeMarketComponent<P, R, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    R: Send + Sync + Clone + 'static,
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
    fn clone(&self) -> Self {
        Self {
            signer_initializer: self.signer_initializer.clone(),
            market_state_preload: self.market_state_preload.clone(),
            price_component: self.price_component.clone(),
            account_monitor: self.account_monitor.clone(),
            protocol_loader: self.protocol_loader.clone(),
            history_loader: self.history_loader.clone(),
        }
    }
}

impl<P, R, DB> Component for CompositeMarketComponent<P, R, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
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
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        // Spawn each component directly
        if let Some(initializer) = self.signer_initializer {
            initializer.spawn(executor.clone())?;
        }
        self.market_state_preload.spawn(executor.clone())?;
        self.price_component.spawn(executor.clone())?;
        self.account_monitor.spawn(executor.clone())?;
        self.protocol_loader.spawn(executor.clone())?;
        self.history_loader.spawn(executor)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "CompositeMarketComponent"
    }
}

impl<N, R, P, DB, Evm> MarketBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuMarketBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Market = CompositeMarketComponent<P, R, DB>;

    async fn build_market(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Market> {
        let kabu_ctx = ctx.as_kabu_context()?;

        // Signer initialization is handled externally (in main or test runner)
        let signer_initializer = None;

        // Market state preloader (one-shot)
        let market_state_preload = MarketStatePreloadedOneShotComponent::<P, Ethereum, DB>::new(kabu_ctx.provider.clone())
            .with_copied_account(kabu_ctx.swap_encoder.get_contract_address())
            .with_signers(kabu_ctx.channels.signers.clone())
            .with_market_state(kabu_ctx.market_state.clone());

        // Price component (one-shot)
        let price_component = PriceComponent::new(kabu_ctx.provider.clone()).only_once().with_market(kabu_ctx.market.clone());

        // Account monitor - uses regular provider for queries and reth provider for canonical state
        let account_monitor = AccountMonitorComponent::<P, R, Ethereum, KabuDataTypesEthereum>::new(
            kabu_ctx.provider.clone(),
            kabu_ctx.channels.account_state.clone(),
            kabu_ctx.channels.signers.clone(),
            std::time::Duration::from_secs(1),
        )
        .with_reth_provider(kabu_ctx.reth_provider.clone());

        // Pool loaders
        let pool_loaders = Arc::new(PoolLoadersBuilder::<_, _, KabuDataTypesEthereum>::default_pool_loaders(
            kabu_ctx.provider.clone(),
            kabu_ctx.pools_config.clone(),
        ));

        // Protocol pool loader
        let protocol_loader = ProtocolPoolLoaderComponent::<P, P, Ethereum>::new(kabu_ctx.provider.clone(), pool_loaders.clone());

        // History pool loader
        let history_loader = HistoryPoolLoaderComponent::<P, P, Ethereum>::new(
            kabu_ctx.provider.clone(),
            pool_loaders,
            0,    // start_block
            1000, // block_batch_size
            10,   // max_batches
        );

        Ok(CompositeMarketComponent {
            signer_initializer,
            market_state_preload,
            price_component,
            account_monitor,
            protocol_loader,
            history_loader,
        })
    }
}

// ================================================================================================
// Broadcaster Builder - Handles flashbots and mempool broadcasting
// ================================================================================================

#[derive(Clone)]
pub struct KabuBroadcasterBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuBroadcasterBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuBroadcasterBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> BroadcasterBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuBroadcasterBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Broadcaster = FlashbotsBroadcastComponent;

    async fn build_broadcaster(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Broadcaster> {
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
pub struct KabuEstimatorBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuEstimatorBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuEstimatorBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> EstimatorBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuEstimatorBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Estimator = EvmEstimatorComponent<P, Ethereum, MulticallerSwapEncoder, DB>;

    async fn build_estimator(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Estimator> {
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
pub struct KabuHealthMonitorBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuHealthMonitorBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuHealthMonitorBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> HealthMonitorBuilderTrait<KabuBuildContext<N, R, P, DB, Evm>> for KabuHealthMonitorBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    #[cfg(feature = "defi-health-monitor")]
    type HealthMonitor = PoolHealthMonitorComponent;

    #[cfg(not(feature = "defi-health-monitor"))]
    type HealthMonitor = PlaceholderComponent;

    async fn build_health_monitor(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::HealthMonitor> {
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
pub struct KabuMergerBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuMergerBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuMergerBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

/// Composite merger component that manages all merger sub-components
pub struct CompositeMergerComponent<N, R, P, DB>
where
    N: reth_node_types::NodeTypesWithDB,
    R: reth_provider::BlockNumReader + reth_provider::HeaderProvider<Header = reth_node_types::HeaderTy<N>> + Send + Sync + Clone + 'static,
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
    swap_path_merger: ArbSwapPathMergerComponent<N, R, DB>,
    same_path_merger: SamePathMergerComponent<P, Ethereum, DB>,
    diff_path_merger: DiffPathMergerComponent<DB>,
}

impl<N, R, P, DB> Clone for CompositeMergerComponent<N, R, P, DB>
where
    N: reth_node_types::NodeTypesWithDB,
    R: reth_provider::BlockNumReader + reth_provider::HeaderProvider<Header = reth_node_types::HeaderTy<N>> + Send + Sync + Clone + 'static,
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
    fn clone(&self) -> Self {
        Self {
            swap_path_merger: self.swap_path_merger.clone(),
            same_path_merger: self.same_path_merger.clone(),
            diff_path_merger: self.diff_path_merger.clone(),
        }
    }
}

impl<N, R, P, DB> Component for CompositeMergerComponent<N, R, P, DB>
where
    N: reth_node_types::NodeTypesWithDB,
    R: reth_provider::BlockNumReader + reth_provider::HeaderProvider<Header = reth_node_types::HeaderTy<N>> + Send + Sync + Clone + 'static,
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
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        // Spawn each component directly
        self.swap_path_merger.spawn(executor.clone())?;
        self.same_path_merger.spawn(executor.clone())?;
        self.diff_path_merger.spawn(executor)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "CompositeMergerComponent"
    }
}

impl<N, R, P, DB, Evm> MergerBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuMergerBuilder<R, P, DB>
where
    N: reth_node_types::NodeTypesWithDB,
    R: reth_provider::BlockNumReader + reth_provider::HeaderProvider<Header = reth_node_types::HeaderTy<N>> + Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Merger = CompositeMergerComponent<N, R, P, DB>;

    async fn build_merger(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Merger> {
        let kabu_ctx = ctx.as_kabu_context()?;

        // Get the swap compose channel from the channels
        let swap_compose_channel = kabu_ctx.channels.swap_compose.clone();

        // Swap path merger - now uses reth_provider instead of latest_block
        let swap_path_merger = ArbSwapPathMergerComponent::<N, _, _>::new(kabu_ctx.reth_provider.clone(), kabu_ctx.multicaller_address)
            .with_market_events_channel(kabu_ctx.channels.market_events.clone())
            .with_compose_channel(swap_compose_channel.clone());

        // Same path merger
        let same_path_merger = SamePathMergerComponent::<_, _, DB>::new(kabu_ctx.provider.clone())
            .with_market_state(kabu_ctx.market_state.clone())
            .with_market_events_channel(kabu_ctx.channels.market_events.clone())
            .with_compose_channel(swap_compose_channel.clone());

        // Diff path merger
        let diff_path_merger = DiffPathMergerComponent::<DB>::new()
            .with_market_events_channel(kabu_ctx.channels.market_events.clone())
            .with_compose_channel(swap_compose_channel);

        Ok(CompositeMergerComponent { swap_path_merger, same_path_merger, diff_path_merger })
    }
}

// ================================================================================================
// Web Server Builder - Handles web server component
// ================================================================================================

#[derive(Clone)]
pub struct KabuWebServerBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuWebServerBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuWebServerBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> WebServerBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuWebServerBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type WebServer = PlaceholderComponent;

    async fn build_web_server(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::WebServer> {
        let kabu_ctx = ctx.as_kabu_context()?;

        // For now, always return a placeholder component
        // In the future, we can conditionally return a real web server based on enable_web_server flag
        if !kabu_ctx.enable_web_server {
            return Ok(PlaceholderComponent::new("WebServer"));
        }

        // Even when enabled, we return placeholder for now since WebServerComponent
        // has complex trait bounds that make it hard to use in a generic context
        Ok(PlaceholderComponent::new("WebServer"))
    }
}

// ================================================================================================
// Monitoring Builder - Handles InfluxDB and other monitoring components
// ================================================================================================

/// Composite monitoring component that manages monitoring sub-components
#[derive(Clone)]
pub struct CompositeMonitoringComponent {
    influxdb_writer: Option<InfluxDbWriterComponent>,
}

impl CompositeMonitoringComponent {
    pub fn new(influxdb_writer: Option<InfluxDbWriterComponent>) -> Self {
        Self { influxdb_writer }
    }
}

impl Component for CompositeMonitoringComponent {
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> Result<()> {
        if let Some(influxdb_writer) = self.influxdb_writer {
            influxdb_writer.spawn(executor)?;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "CompositeMonitoringComponent"
    }
}

#[derive(Clone)]
pub struct KabuMonitoringBuilder<R, P, DB> {
    _phantom: PhantomData<(R, P, DB)>,
}

impl<R, P, DB> Default for KabuMonitoringBuilder<R, P, DB> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB> KabuMonitoringBuilder<R, P, DB> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> MonitoringBuilder<KabuBuildContext<N, R, P, DB, Evm>> for KabuMonitoringBuilder<R, P, DB>
where
    N: NodeTypesWithDB,
    R: Send + Sync + Clone + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Monitoring = CompositeMonitoringComponent;

    async fn build_monitoring(self, ctx: &BuilderContext<KabuBuildContext<N, R, P, DB, Evm>>) -> Result<Self::Monitoring> {
        let kabu_ctx = ctx.as_kabu_context()?;

        let influxdb_writer = kabu_ctx.topology_config.influxdb.as_ref().map(|influxdb_config| {
            InfluxDbWriterComponent::new(influxdb_config.url.clone(), influxdb_config.database.clone(), influxdb_config.tags.clone())
                .with_channel(kabu_ctx.blockchain.influxdb_write_channel())
        });

        Ok(CompositeMonitoringComponent::new(influxdb_writer))
    }
}

// ================================================================================================
// Kabu Ethereum Node Components Builder
// ================================================================================================

/// Components builder for Kabu Ethereum node
#[derive(Clone)]
pub struct KabuEthereumComponentsBuilder<N, R, P, DB> {
    _phantom: PhantomData<(N, R, P, DB)>,
}

impl<N, R, P, DB> Default for KabuEthereumComponentsBuilder<N, R, P, DB> {
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<N, R, P, DB, Evm> KabuNodeComponentsBuilder<KabuEthereumNode<N, R, P, DB, Evm>> for KabuEthereumComponentsBuilder<N, R, P, DB>
where
    N: reth_node_types::NodeTypesWithDB,
    R: reth_provider::BlockNumReader
        + reth_provider::HeaderProvider<Header = reth_node_types::HeaderTy<N>>
        + CanonStateSubscriptions<Primitives = EthPrimitives>
        + Send
        + Sync
        + Clone
        + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type Pool = KabuPoolBuilder<R, P, DB>;
    type Network = KabuNetworkBuilder<R, P, DB>;
    type Executor = KabuExecutorBuilder<R, P, DB>;
    type Strategy = KabuStrategyBuilder<R, P, DB>;
    type Signer = KabuSignerBuilder<R, P, DB>;
    type Market = KabuMarketBuilder<R, P, DB>;
    type Broadcaster = KabuBroadcasterBuilder<R, P, DB>;
    type Estimator = KabuEstimatorBuilder<R, P, DB>;
    type HealthMonitor = KabuHealthMonitorBuilder<R, P, DB>;
    type Merger = KabuMergerBuilder<R, P, DB>;
    type WebServer = KabuWebServerBuilder<R, P, DB>;
    type Monitoring = KabuMonitoringBuilder<R, P, DB>;

    fn build_components(self) -> KabuComponentsSet<KabuBuildContext<N, R, P, DB, Evm>, Self, KabuEthereumNode<N, R, P, DB, Evm>> {
        KabuComponentsSet {
            pool: KabuPoolBuilder::new(),
            network: KabuNetworkBuilder::new(),
            executor: KabuExecutorBuilder::new(),
            strategy: KabuStrategyBuilder::new(),
            signer: KabuSignerBuilder::new(),
            market: KabuMarketBuilder::new(),
            broadcaster: KabuBroadcasterBuilder::new(),
            estimator: KabuEstimatorBuilder::new(),
            health_monitor: KabuHealthMonitorBuilder::new(),
            merger: KabuMergerBuilder::new(),
            web_server: KabuWebServerBuilder::new(),
            monitoring: KabuMonitoringBuilder::new(),
            _phantom: PhantomData,
        }
    }
}

// Implement KabuNode trait for KabuEthereumNode
impl<N, R, P, DB, Evm> KabuNodeTrait<KabuEthereumNode<N, R, P, DB, Evm>> for KabuEthereumNode<N, R, P, DB, Evm>
where
    N: NodeTypesWithDB,
    R: reth_provider::BlockNumReader
        + reth_provider::HeaderProvider<Header = reth_node_types::HeaderTy<N>>
        + CanonStateSubscriptions<Primitives = EthPrimitives>
        + Send
        + Sync
        + Clone
        + 'static,
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
    Evm: ConfigureEvm + 'static,
{
    type ComponentsBuilder = KabuEthereumComponentsBuilder<N, R, P, DB>;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        KabuEthereumComponentsBuilder::default()
    }
}
