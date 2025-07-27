use crate::{ComponentsBuilder, ExecutorBuilder, NetworkBuilder, PoolBuilder, StrategyBuilder};

/// Trait for defining a complete node configuration
pub trait Node<State>: Clone + Send + Sync + 'static {
    /// The components builder type
    type ComponentsBuilder: NodeComponents<State>;

    /// Get the components builder
    fn components(&self) -> Self::ComponentsBuilder;
}

/// Trait for node components configuration
pub trait NodeComponents<State>: Clone + Send + Sync + 'static {
    /// Pool builder type
    type Pool: PoolBuilder<State>;
    /// Network builder type
    type Network: NetworkBuilder<State>;
    /// Executor builder type
    type Executor: ExecutorBuilder<State>;
    /// Strategy builder type
    type Strategy: StrategyBuilder<State>;

    /// Create a ComponentsBuilder with the configured builders
    fn build(self) -> ComponentsBuilder<State, Self::Pool, Self::Network, Self::Executor, Self::Strategy>;
}

/// Default implementation of NodeComponents
#[derive(Clone)]
pub struct DefaultNodeComponents<Pool, Network, Executor, Strategy> {
    pool: Pool,
    network: Network,
    executor: Executor,
    strategy: Strategy,
}

impl<Pool, Network, Executor, Strategy> DefaultNodeComponents<Pool, Network, Executor, Strategy> {
    pub fn new(pool: Pool, network: Network, executor: Executor, strategy: Strategy) -> Self {
        Self { pool, network, executor, strategy }
    }
}

impl<State, Pool, Network, Executor, Strategy> NodeComponents<State> for DefaultNodeComponents<Pool, Network, Executor, Strategy>
where
    State: Clone + Send + Sync + 'static,
    Pool: PoolBuilder<State> + Clone + Send + Sync + 'static,
    Network: NetworkBuilder<State> + Clone + Send + Sync + 'static,
    Executor: ExecutorBuilder<State> + Clone + Send + Sync + 'static,
    Strategy: StrategyBuilder<State> + Clone + Send + Sync + 'static,
{
    type Pool = Pool;
    type Network = Network;
    type Executor = Executor;
    type Strategy = Strategy;

    fn build(self) -> ComponentsBuilder<State, Self::Pool, Self::Network, Self::Executor, Self::Strategy> {
        ComponentsBuilder::default().pool(self.pool).network(self.network).executor(self.executor).strategy(self.strategy)
    }
}
