use crate::traits::{
    BroadcasterBuilder, EstimatorBuilder, ExecutorBuilder, HealthMonitorBuilder, MarketBuilder, MergerBuilder, MonitoringBuilder,
    NetworkBuilder, PoolBuilder, SignerBuilder, StrategyBuilder, WebServerBuilder,
};

/// Kabu node types configuration
pub trait KabuNodeTypes: Clone + Send + Sync + 'static {
    /// The state type for the node
    type State: Clone + Send + Sync + 'static;
}

/// Trait for defining a complete Kabu node configuration
pub trait KabuNode<N: KabuNodeTypes>: KabuNodeTypes + Clone {
    /// The type that builds the node's components
    type ComponentsBuilder: KabuNodeComponentsBuilder<N>;

    /// Returns a [`KabuNodeComponentsBuilder`] for the node
    fn components_builder(&self) -> Self::ComponentsBuilder;
}

/// Trait for Kabu node components builder configuration
pub trait KabuNodeComponentsBuilder<N: KabuNodeTypes>: Clone + Send + Sync + 'static {
    /// Pool/mempool builder type
    type Pool: PoolBuilder<N::State>;
    /// Network connectivity builder type
    type Network: NetworkBuilder<N::State>;
    /// Transaction executor/router builder type
    type Executor: ExecutorBuilder<N::State>;
    /// MEV strategy builder type (arbitrage, liquidation, etc.)
    type Strategy: StrategyBuilder<N::State>;
    /// Transaction signer builder type
    type Signer: SignerBuilder<N::State>;
    /// Market data builder type (pools, tokens, prices)
    type Market: MarketBuilder<N::State>;
    /// Transaction broadcaster builder type (flashbots, mempool)
    type Broadcaster: BroadcasterBuilder<N::State>;
    /// Gas/profit estimator builder type
    type Estimator: EstimatorBuilder<N::State>;
    /// Health monitoring builder type
    type HealthMonitor: HealthMonitorBuilder<N::State>;
    /// Merger builder type (combines strategies/paths)
    type Merger: MergerBuilder<N::State>;
    /// Web server builder type
    type WebServer: WebServerBuilder<N::State>;
    /// Monitoring builder type (metrics, influxdb)
    type Monitoring: MonitoringBuilder<N::State>;

    /// Build and return all component builders
    fn build_components(self) -> KabuComponentsSet<N::State, Self, N>;
}

/// Extension trait for adding node configuration to a builder
pub trait WithKabuNode<B> {
    /// Configure the builder with a Kabu node
    fn node<N>(self, node: N) -> B
    where
        N: KabuNode<N> + KabuNodeTypes;
}

/// Set of all Kabu component builders
pub struct KabuComponentsSet<State, Builder, N>
where
    N: KabuNodeTypes<State = State>,
    Builder: KabuNodeComponentsBuilder<N>,
{
    pub pool: Builder::Pool,
    pub network: Builder::Network,
    pub executor: Builder::Executor,
    pub strategy: Builder::Strategy,
    pub signer: Builder::Signer,
    pub market: Builder::Market,
    pub broadcaster: Builder::Broadcaster,
    pub estimator: Builder::Estimator,
    pub health_monitor: Builder::HealthMonitor,
    pub merger: Builder::Merger,
    pub web_server: Builder::WebServer,
    pub monitoring: Builder::Monitoring,
    pub _phantom: std::marker::PhantomData<N>,
}

/// Default implementation of KabuNodeComponentsBuilder
#[derive(Clone)]
pub struct DefaultKabuNodeComponentsBuilder<
    Pool,
    Network,
    Executor,
    Strategy,
    Signer,
    Market,
    Broadcaster,
    Estimator,
    HealthMonitor,
    Merger,
    WebServer,
    Monitoring,
> {
    pool: Pool,
    network: Network,
    executor: Executor,
    strategy: Strategy,
    signer: Signer,
    market: Market,
    broadcaster: Broadcaster,
    estimator: Estimator,
    health_monitor: HealthMonitor,
    merger: Merger,
    web_server: WebServer,
    monitoring: Monitoring,
}

impl<Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor, Merger, WebServer, Monitoring, N>
    KabuNodeComponentsBuilder<N>
    for DefaultKabuNodeComponentsBuilder<
        Pool,
        Network,
        Executor,
        Strategy,
        Signer,
        Market,
        Broadcaster,
        Estimator,
        HealthMonitor,
        Merger,
        WebServer,
        Monitoring,
    >
where
    N: KabuNodeTypes,
    Pool: PoolBuilder<N::State> + Clone + Send + Sync + 'static,
    Network: NetworkBuilder<N::State> + Clone + Send + Sync + 'static,
    Executor: ExecutorBuilder<N::State> + Clone + Send + Sync + 'static,
    Strategy: StrategyBuilder<N::State> + Clone + Send + Sync + 'static,
    Signer: SignerBuilder<N::State> + Clone + Send + Sync + 'static,
    Market: MarketBuilder<N::State> + Clone + Send + Sync + 'static,
    Broadcaster: BroadcasterBuilder<N::State> + Clone + Send + Sync + 'static,
    Estimator: EstimatorBuilder<N::State> + Clone + Send + Sync + 'static,
    HealthMonitor: HealthMonitorBuilder<N::State> + Clone + Send + Sync + 'static,
    Merger: MergerBuilder<N::State> + Clone + Send + Sync + 'static,
    WebServer: WebServerBuilder<N::State> + Clone + Send + Sync + 'static,
    Monitoring: MonitoringBuilder<N::State> + Clone + Send + Sync + 'static,
{
    type Pool = Pool;
    type Network = Network;
    type Executor = Executor;
    type Strategy = Strategy;
    type Signer = Signer;
    type Market = Market;
    type Broadcaster = Broadcaster;
    type Estimator = Estimator;
    type HealthMonitor = HealthMonitor;
    type Merger = Merger;
    type WebServer = WebServer;
    type Monitoring = Monitoring;

    fn build_components(self) -> KabuComponentsSet<N::State, Self, N> {
        KabuComponentsSet {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            signer: self.signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            merger: self.merger,
            web_server: self.web_server,
            monitoring: self.monitoring,
            _phantom: std::marker::PhantomData,
        }
    }
}
