use crate::traits::{
    BroadcasterBuilder, EstimatorBuilder, ExecutorBuilder, HealthMonitorBuilder, MarketBuilder, NetworkBuilder, PoolBuilder, SignerBuilder,
    StrategyBuilder,
};
use crate::Component;
use eyre::Result;
use reth_tasks::TaskExecutor;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

// Import MEV-specific types for channel communication
use kabu_evm_db::KabuDB;
use kabu_types_entities::{AccountNonceAndBalanceState, TxSigners};
use kabu_types_events::{MarketEvents, MempoolEvents, MessageHealthEvent, MessageSwapCompose, MessageTxCompose};
use reth_ethereum_primitives::EthPrimitives;

/// MEV Component Communication Channels
/// This struct holds all the broadcast channels used for inter-component communication
#[derive(Clone)]
pub struct MevComponentChannels<DB = KabuDB> {
    /// Channel for swap composition messages (Strategy -> Router -> Signer)
    pub swap_compose: broadcast::Sender<MessageSwapCompose<DB, EthPrimitives>>,
    /// Channel for transaction composition messages (Signer -> Broadcaster)
    pub tx_compose: broadcast::Sender<MessageTxCompose<EthPrimitives>>,
    /// Channel for market events (Market -> Strategy/HealthMonitor)
    pub market_events: broadcast::Sender<MarketEvents>,
    /// Channel for mempool events (Pool -> Strategy)
    pub mempool_events: broadcast::Sender<MempoolEvents>,
    /// Channel for health monitoring events (HealthMonitor -> Market/Strategy)
    pub health_events: broadcast::Sender<MessageHealthEvent>,
    /// Shared signers state (used by Router and Signer)
    pub signers: Arc<RwLock<TxSigners<EthPrimitives>>>,
    /// Account nonce and balance tracking (used by Signer and Broadcaster)
    pub account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
}

impl<DB: Clone> Default for MevComponentChannels<DB> {
    fn default() -> Self {
        let (swap_compose, _) = broadcast::channel(1000);
        let (tx_compose, _) = broadcast::channel(1000);
        let (market_events, _) = broadcast::channel(1000);
        let (mempool_events, _) = broadcast::channel(1000);
        let (health_events, _) = broadcast::channel(1000);

        Self {
            swap_compose,
            tx_compose,
            market_events,
            mempool_events,
            health_events,
            signers: Arc::new(RwLock::new(TxSigners::<EthPrimitives>::new())),
            account_state: Arc::new(RwLock::new(AccountNonceAndBalanceState::new())),
        }
    }
}

/// Context provided to component builders
#[derive(Clone)]
pub struct BuilderContext<State, DB = KabuDB> {
    pub state: State,
    pub channels: MevComponentChannels<DB>,
}

impl<State, DB: Clone> BuilderContext<State, DB> {
    pub fn new(state: State) -> Self {
        Self { state, channels: MevComponentChannels::default() }
    }

    pub fn with_channels(state: State, channels: MevComponentChannels<DB>) -> Self {
        Self { state, channels }
    }
}

/// Generic components builder
pub struct ComponentsBuilder<State, Pool = (), Network = (), Executor = (), Strategy = ()> {
    pool: Pool,
    network: Network,
    executor: Executor,
    strategy: Strategy,
    _state: PhantomData<State>,
}

impl<State> Default for ComponentsBuilder<State> {
    fn default() -> Self {
        Self { pool: (), network: (), executor: (), strategy: (), _state: PhantomData }
    }
}

impl<State> ComponentsBuilder<State> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<State, Pool, Network, Executor, Strategy> ComponentsBuilder<State, Pool, Network, Executor, Strategy> {
    /// Set the pool builder
    pub fn pool<P>(self, pool: P) -> ComponentsBuilder<State, P, Network, Executor, Strategy> {
        ComponentsBuilder { pool, network: self.network, executor: self.executor, strategy: self.strategy, _state: PhantomData }
    }

    /// Set the network builder
    pub fn network<N>(self, network: N) -> ComponentsBuilder<State, Pool, N, Executor, Strategy> {
        ComponentsBuilder { pool: self.pool, network, executor: self.executor, strategy: self.strategy, _state: PhantomData }
    }

    /// Set the executor builder
    pub fn executor<E>(self, executor: E) -> ComponentsBuilder<State, Pool, Network, E, Strategy> {
        ComponentsBuilder { pool: self.pool, network: self.network, executor, strategy: self.strategy, _state: PhantomData }
    }

    /// Set the strategy builder
    pub fn strategy<S>(self, strategy: S) -> ComponentsBuilder<State, Pool, Network, Executor, S> {
        ComponentsBuilder { pool: self.pool, network: self.network, executor: self.executor, strategy, _state: PhantomData }
    }

    /// Map the pool builder
    pub fn map_pool<F, P>(self, f: F) -> ComponentsBuilder<State, P, Network, Executor, Strategy>
    where
        F: FnOnce(Pool) -> P,
    {
        ComponentsBuilder {
            pool: f(self.pool),
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            _state: PhantomData,
        }
    }

    /// Map the network builder
    pub fn map_network<F, N>(self, f: F) -> ComponentsBuilder<State, Pool, N, Executor, Strategy>
    where
        F: FnOnce(Network) -> N,
    {
        ComponentsBuilder {
            pool: self.pool,
            network: f(self.network),
            executor: self.executor,
            strategy: self.strategy,
            _state: PhantomData,
        }
    }

    /// Map the executor builder
    pub fn map_executor<F, E>(self, f: F) -> ComponentsBuilder<State, Pool, Network, E, Strategy>
    where
        F: FnOnce(Executor) -> E,
    {
        ComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: f(self.executor),
            strategy: self.strategy,
            _state: PhantomData,
        }
    }

    /// Map the strategy builder
    pub fn map_strategy<F, S>(self, f: F) -> ComponentsBuilder<State, Pool, Network, Executor, S>
    where
        F: FnOnce(Strategy) -> S,
    {
        ComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy: f(self.strategy),
            _state: PhantomData,
        }
    }
}

impl<State, Pool, Network, Executor, Strategy> ComponentsBuilder<State, Pool, Network, Executor, Strategy>
where
    State: Clone + Send + Sync + 'static,
    Pool: PoolBuilder<State>,
    Network: NetworkBuilder<State>,
    Executor: ExecutorBuilder<State>,
    Strategy: StrategyBuilder<State>,
{
    /// Build all components and spawn them
    pub async fn build_and_spawn(self, ctx: BuilderContext<State>, executor: TaskExecutor) -> Result<()> {
        // Build components in dependency order
        let pool = self.pool.build_pool(&ctx).await?;
        let network = self.network.build_network(&ctx).await?;
        let exec_comp = self.executor.build_executor(&ctx).await?;
        let strategy = self.strategy.build_strategy(&ctx).await?;

        // Spawn components
        pool.spawn(executor.clone())?;
        network.spawn(executor.clone())?;
        exec_comp.spawn(executor.clone())?;
        strategy.spawn(executor)?;

        Ok(())
    }

    /// Build all components without spawning
    pub async fn build(
        self,
        ctx: BuilderContext<State>,
    ) -> Result<Components<Pool::Pool, Network::Network, Executor::Executor, Strategy::Strategy>> {
        let pool = self.pool.build_pool(&ctx).await?;
        let network = self.network.build_network(&ctx).await?;
        let executor = self.executor.build_executor(&ctx).await?;
        let strategy = self.strategy.build_strategy(&ctx).await?;

        Ok(Components::new(pool, network, executor, strategy))
    }
}

/// Components bundle that holds all built components
pub struct Components<Pool, Network, Executor, Strategy> {
    pub pool: Pool,
    pub network: Network,
    pub executor: Executor,
    pub strategy: Strategy,
}

impl<Pool, Network, Executor, Strategy> Components<Pool, Network, Executor, Strategy> {
    pub fn new(pool: Pool, network: Network, executor: Executor, strategy: Strategy) -> Self {
        Self { pool, network, executor, strategy }
    }
}

impl<Pool, Network, Executor, Strategy> Components<Pool, Network, Executor, Strategy>
where
    Pool: Component,
    Network: Component,
    Executor: Component,
    Strategy: Component,
{
    /// Spawn all components
    pub fn spawn_all(self, executor: TaskExecutor) -> Result<()> {
        self.pool.spawn(executor.clone())?;
        self.network.spawn(executor.clone())?;
        self.executor.spawn(executor.clone())?;
        self.strategy.spawn(executor)?;
        Ok(())
    }
}

/// Comprehensive MEV components builder for full MEV bot functionality
pub struct MevComponentsBuilder<
    State,
    Pool = (),
    Network = (),
    Executor = (),
    Strategy = (),
    Signer = (),
    Market = (),
    Broadcaster = (),
    Estimator = (),
    HealthMonitor = (),
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
    _state: PhantomData<State>,
}

impl<State> Default for MevComponentsBuilder<State> {
    fn default() -> Self {
        Self {
            pool: (),
            network: (),
            executor: (),
            strategy: (),
            signer: (),
            market: (),
            broadcaster: (),
            estimator: (),
            health_monitor: (),
            _state: PhantomData,
        }
    }
}

impl<State> MevComponentsBuilder<State> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<State, Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor>
    MevComponentsBuilder<State, Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor>
{
    /// Set the pool builder
    pub fn pool<P>(
        self,
        pool: P,
    ) -> MevComponentsBuilder<State, P, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor> {
        MevComponentsBuilder {
            pool,
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            signer: self.signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the network builder
    pub fn network<N>(
        self,
        network: N,
    ) -> MevComponentsBuilder<State, Pool, N, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor> {
        MevComponentsBuilder {
            pool: self.pool,
            network,
            executor: self.executor,
            strategy: self.strategy,
            signer: self.signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the executor builder
    pub fn executor<E>(
        self,
        executor: E,
    ) -> MevComponentsBuilder<State, Pool, Network, E, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor> {
        MevComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor,
            strategy: self.strategy,
            signer: self.signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the strategy builder
    pub fn strategy<S>(
        self,
        strategy: S,
    ) -> MevComponentsBuilder<State, Pool, Network, Executor, S, Signer, Market, Broadcaster, Estimator, HealthMonitor> {
        MevComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy,
            signer: self.signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the signer builder
    pub fn signer<SI>(
        self,
        signer: SI,
    ) -> MevComponentsBuilder<State, Pool, Network, Executor, Strategy, SI, Market, Broadcaster, Estimator, HealthMonitor> {
        MevComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the market builder
    pub fn market<M>(
        self,
        market: M,
    ) -> MevComponentsBuilder<State, Pool, Network, Executor, Strategy, Signer, M, Broadcaster, Estimator, HealthMonitor> {
        MevComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            signer: self.signer,
            market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the broadcaster builder
    pub fn broadcaster<B>(
        self,
        broadcaster: B,
    ) -> MevComponentsBuilder<State, Pool, Network, Executor, Strategy, Signer, Market, B, Estimator, HealthMonitor> {
        MevComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            signer: self.signer,
            market: self.market,
            broadcaster,
            estimator: self.estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the estimator builder
    pub fn estimator<ES>(
        self,
        estimator: ES,
    ) -> MevComponentsBuilder<State, Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, ES, HealthMonitor> {
        MevComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            signer: self.signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator,
            health_monitor: self.health_monitor,
            _state: PhantomData,
        }
    }

    /// Set the health monitor builder
    pub fn health_monitor<H>(
        self,
        health_monitor: H,
    ) -> MevComponentsBuilder<State, Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, H> {
        MevComponentsBuilder {
            pool: self.pool,
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            signer: self.signer,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health_monitor,
            _state: PhantomData,
        }
    }
}

impl<State, Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor>
    MevComponentsBuilder<State, Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor>
where
    State: Clone + Send + Sync + 'static,
    Pool: PoolBuilder<State>,
    Network: NetworkBuilder<State>,
    Executor: ExecutorBuilder<State>,
    Strategy: StrategyBuilder<State>,
    Signer: SignerBuilder<State>,
    Market: MarketBuilder<State>,
    Broadcaster: BroadcasterBuilder<State>,
    Estimator: EstimatorBuilder<State>,
    HealthMonitor: HealthMonitorBuilder<State>,
{
    /// Build all MEV components and spawn them
    pub async fn build_and_spawn(self, ctx: BuilderContext<State>, executor: TaskExecutor) -> Result<()> {
        // Build components in dependency order
        let pool = self.pool.build_pool(&ctx).await?;
        let network = self.network.build_network(&ctx).await?;
        let exec_comp = self.executor.build_executor(&ctx).await?;
        let strategy = self.strategy.build_strategy(&ctx).await?;
        let signer = self.signer.build_signer(&ctx).await?;
        let market = self.market.build_market(&ctx).await?;
        let broadcaster = self.broadcaster.build_broadcaster(&ctx).await?;
        let estimator = self.estimator.build_estimator(&ctx).await?;
        let health_monitor = self.health_monitor.build_health_monitor(&ctx).await?;

        // Spawn components in dependency order
        market.spawn(executor.clone())?; // Core market data first
        pool.spawn(executor.clone())?; // Pool/mempool access
        network.spawn(executor.clone())?; // Network connectivity
        estimator.spawn(executor.clone())?; // Gas/profit estimation
        strategy.spawn(executor.clone())?; // MEV strategy detection
        exec_comp.spawn(executor.clone())?; // Transaction routing/encoding
        signer.spawn(executor.clone())?; // Transaction signing
        broadcaster.spawn(executor.clone())?; // Transaction broadcasting
        health_monitor.spawn(executor)?; // Health monitoring last

        Ok(())
    }

    /// Build all MEV components without spawning (Reth-style component wiring)
    pub async fn build(
        self,
        ctx: BuilderContext<State>,
    ) -> Result<
        MevComponents<
            Pool::Pool,
            Network::Network,
            Executor::Executor,
            Strategy::Strategy,
            Signer::Signer,
            Market::Market,
            Broadcaster::Broadcaster,
            Estimator::Estimator,
            HealthMonitor::HealthMonitor,
        >,
    > {
        let Self {
            pool: pool_builder,
            network: network_builder,
            executor: executor_builder,
            strategy: strategy_builder,
            signer: signer_builder,
            market: market_builder,
            broadcaster: broadcaster_builder,
            estimator: estimator_builder,
            health_monitor: health_monitor_builder,
            _state,
        } = self;

        // Build components in dependency order:
        // 1. First build standalone components (market, pools)
        // 2. Then build components that depend on shared state (signers, account state)
        // 3. Finally build service components that wire everything together

        // Step 1: Build market and shared state components
        let market = market_builder.build_market(&ctx).await?;

        // Step 2: Build pool component (needs market state access)
        let pool = pool_builder.build_pool(&ctx).await?;

        // Step 3: Build network component (may need pool for mempool integration)
        let network = network_builder.build_network(&ctx).await?;

        // Step 4: Build estimation component (needs market access)
        let estimator = estimator_builder.build_estimator(&ctx).await?;

        // Step 5: Build strategy component (needs market, mempool channels)
        let strategy = strategy_builder.build_strategy(&ctx).await?;

        // Step 6: Build executor/router component (needs strategy output, signer access)
        let executor = executor_builder.build_executor(&ctx).await?;

        // Step 7: Build signer component (needs executor output)
        let signer = signer_builder.build_signer(&ctx).await?;

        // Step 8: Build broadcaster component (needs signed transactions)
        let broadcaster = broadcaster_builder.build_broadcaster(&ctx).await?;

        // Step 9: Build health monitor (observes all other components)
        let health_monitor = health_monitor_builder.build_health_monitor(&ctx).await?;

        Ok(MevComponents { pool, network, executor, strategy, signer, market, broadcaster, estimator, health_monitor })
    }
}

/// MEV components bundle that holds all built MEV components
pub struct MevComponents<Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor> {
    pub pool: Pool,
    pub network: Network,
    pub executor: Executor,
    pub strategy: Strategy,
    pub signer: Signer,
    pub market: Market,
    pub broadcaster: Broadcaster,
    pub estimator: Estimator,
    pub health_monitor: HealthMonitor,
}

impl<Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor>
    MevComponents<Pool, Network, Executor, Strategy, Signer, Market, Broadcaster, Estimator, HealthMonitor>
where
    Pool: Component,
    Network: Component,
    Executor: Component,
    Strategy: Component,
    Signer: Component,
    Market: Component,
    Broadcaster: Component,
    Estimator: Component,
    HealthMonitor: Component,
{
    /// Spawn all MEV components
    pub fn spawn_all(self, executor: TaskExecutor) -> Result<()> {
        self.market.spawn(executor.clone())?;
        self.pool.spawn(executor.clone())?;
        self.network.spawn(executor.clone())?;
        self.estimator.spawn(executor.clone())?;
        self.strategy.spawn(executor.clone())?;
        self.executor.spawn(executor.clone())?;
        self.signer.spawn(executor.clone())?;
        self.broadcaster.spawn(executor.clone())?;
        self.health_monitor.spawn(executor)?;
        Ok(())
    }
}
