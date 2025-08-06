use crate::traits::{
    BroadcasterBuilder, EstimatorBuilder, ExecutorBuilder, HealthMonitorBuilder, MarketBuilder, MergerBuilder, MonitoringBuilder,
    NetworkBuilder, PoolBuilder, SignerBuilder, StrategyBuilder, WebServerBuilder,
};
use crate::{BuilderContext, KabuComponentsSet, KabuNode, KabuNodeComponentsBuilder, KabuNodeTypes, MevComponentChannels};
use eyre::Result;
use reth_tasks::TaskExecutor;
use std::marker::PhantomData;

/// Kabu MEV bot builder - follows Reth's builder pattern
pub struct KabuBuilder<State, Node = ()> {
    state: State,
    node: Node,
}

impl<State> KabuBuilder<State, ()> {
    /// Create a new Kabu builder with the given state
    pub fn new(state: State) -> Self {
        Self { state, node: () }
    }
}

impl<State> KabuBuilder<State, ()> {
    pub fn node<N>(self, node: N) -> KabuBuilder<State, N>
    where
        N: KabuNode<N> + KabuNodeTypes,
    {
        KabuBuilder { state: self.state, node }
    }
}

impl<State, N> KabuBuilder<State, N>
where
    N: KabuNode<N> + KabuNodeTypes<State = State>,
    State: Clone + Send + Sync + 'static,
{
    /// Build the node with the configured components
    pub fn build(self) -> KabuBuiltNode<State, N> {
        KabuBuiltNode { state: self.state, node: self.node }
    }
}

/// A built Kabu node ready to be launched
pub struct KabuBuiltNode<State, N: KabuNode<N> + KabuNodeTypes> {
    state: State,
    node: N,
}

impl<State, N> KabuBuiltNode<State, N>
where
    N: KabuNode<N> + KabuNodeTypes<State = State>,
    State: Clone + Send + Sync + 'static,
{
    /// Launch the node with the given task executor
    pub async fn launch(self, executor: TaskExecutor) -> Result<KabuHandle> {
        // Create channels
        let channels = MevComponentChannels::default();

        // Create builder context
        let context = BuilderContext::with_channels(self.state.clone(), channels.clone());

        // Get component builders from the node
        let components_builder = self.node.components_builder();
        let components_set = components_builder.build_components();

        // Build and spawn all components
        self.spawn_all_components(context, components_set, executor).await?;

        Ok(KabuHandle { _phantom: PhantomData })
    }

    /// Launch with custom channels
    pub async fn launch_with_channels(self, executor: TaskExecutor, channels: MevComponentChannels) -> Result<KabuHandle> {
        // Create builder context with custom channels
        let context = BuilderContext::with_channels(self.state.clone(), channels);

        // Get component builders from the node
        let components_builder = self.node.components_builder();
        let components_set = components_builder.build_components();

        // Build and spawn all components
        self.spawn_all_components(context, components_set, executor).await?;

        Ok(KabuHandle { _phantom: PhantomData })
    }

    async fn spawn_all_components<CB>(
        &self,
        context: BuilderContext<State>,
        components: KabuComponentsSet<State, CB, N>,
        executor: TaskExecutor,
    ) -> Result<()>
    where
        CB: KabuNodeComponentsBuilder<N>,
    {
        // Build components in dependency order
        let pool = components.pool.build_pool(&context).await?;
        let network = components.network.build_network(&context).await?;
        let market = components.market.build_market(&context).await?;
        let estimator = components.estimator.build_estimator(&context).await?;
        let strategy = components.strategy.build_strategy(&context).await?;
        let merger = components.merger.build_merger(&context).await?;
        let executor_comp = components.executor.build_executor(&context).await?;
        let signer = components.signer.build_signer(&context).await?;
        let broadcaster = components.broadcaster.build_broadcaster(&context).await?;
        let health_monitor = components.health_monitor.build_health_monitor(&context).await?;
        let web_server = components.web_server.build_web_server(&context).await?;
        let monitoring = components.monitoring.build_monitoring(&context).await?;

        // Spawn components in dependency order
        use crate::Component;

        market.spawn(executor.clone())?;
        pool.spawn(executor.clone())?;
        network.spawn(executor.clone())?;
        estimator.spawn(executor.clone())?;
        strategy.spawn(executor.clone())?;
        merger.spawn(executor.clone())?;
        executor_comp.spawn(executor.clone())?;
        signer.spawn(executor.clone())?;
        broadcaster.spawn(executor.clone())?;
        health_monitor.spawn(executor.clone())?;
        web_server.spawn(executor.clone())?;
        monitoring.spawn(executor)?;

        Ok(())
    }
}

/// Handle to a running Kabu node
pub struct KabuHandle {
    _phantom: PhantomData<()>,
}

impl KabuHandle {
    /// Wait for the node to shutdown
    pub async fn wait_for_shutdown(self) -> Result<()> {
        // In a real implementation, this would wait for shutdown signals
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}
