//! Node builder for launching Kabu MEV bot with configured components

use crate::context::KabuContext;
use crate::node::ComponentSet;
use crate::traits::ComponentBuilder;
use alloy_network::Ethereum;
use alloy_provider::Provider;
use eyre::Result;
use kabu_core_components::Component;
use kabu_evm_db::{DatabaseKabuExt, KabuDBError};
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_entities::BlockHistoryState;
use reth::revm::{Database, DatabaseCommit, DatabaseRef};
use reth_ethereum_primitives::EthPrimitives;
use reth_tasks::TaskExecutor;
use std::marker::PhantomData;

/// Type alias for a NodeBuilder with configured components
type ConfiguredNodeBuilder<R, P, DB, Evm, N, E, S, M, B, Est, H, Mer, Mon, Sig> =
    NodeBuilder<R, P, DB, Evm, ComponentSet<N, E, S, M, B, Est, H, Mer, Mon, Sig>>;

/// Builder for creating and launching a Kabu node
pub struct NodeBuilder<R, P, DB, Evm, Components = ()>
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
    context: Option<KabuContext<R, P, DB, Evm>>,
    components: Components,
    _phantom: PhantomData<(R, P, DB, Evm)>,
}

impl<R, P, DB, Evm> Default for NodeBuilder<R, P, DB, Evm, ()>
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
    fn default() -> Self {
        Self::new()
    }
}

impl<R, P, DB, Evm> NodeBuilder<R, P, DB, Evm, ()>
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
    /// Create a new node builder
    pub fn new() -> Self {
        Self { context: None, components: (), _phantom: PhantomData }
    }

    /// Set the context for the node
    pub fn with_context(mut self, context: KabuContext<R, P, DB, Evm>) -> Self {
        self.context = Some(context);
        self
    }

    /// Set the components for the node
    #[allow(clippy::type_complexity)]
    pub fn with_components<N, E, S, M, B, Est, H, Mer, Mon, Sig>(
        self,
        components: ComponentSet<N, E, S, M, B, Est, H, Mer, Mon, Sig>,
    ) -> ConfiguredNodeBuilder<R, P, DB, Evm, N, E, S, M, B, Est, H, Mer, Mon, Sig> {
        NodeBuilder { context: self.context, components, _phantom: PhantomData }
    }
}

impl<R, P, DB, Evm, N, E, S, M, B, Est, H, Mer, Mon, Sig> NodeBuilder<R, P, DB, Evm, ComponentSet<N, E, S, M, B, Est, H, Mer, Mon, Sig>>
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
    N: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    E: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    S: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    M: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    B: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    Est: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    H: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    Mer: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    Mon: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
    Sig: ComponentBuilder<KabuContext<R, P, DB, Evm>>,
{
    /// Launch the node with all components
    pub async fn launch(self, executor: TaskExecutor) -> Result<KabuHandle> {
        let context = self.context.expect("Context must be set before launching");

        // Build and spawn all components in dependency order

        // Network component (block history)
        let network = self.components.network.build(&context)?;
        network.spawn(executor.clone())?;

        // Market components (pools, price, etc.)
        let market = self.components.market.build(&context)?;
        market.spawn(executor.clone())?;

        // Executor component (swap router)
        let executor_component = self.components.executor.build(&context)?;
        executor_component.spawn(executor.clone())?;

        // Strategy component (arbitrage searcher)
        let strategy = self.components.strategy.build(&context)?;
        strategy.spawn(executor.clone())?;

        // Estimator component (gas estimation)
        let estimator = self.components.estimator.build(&context)?;
        estimator.spawn(executor.clone())?;

        // Signer bridge component (converts Ready to TxCompose)
        let signer = self.components.signer.build(&context)?;
        signer.spawn(executor.clone())?;

        // Merger components (swap optimization)
        let merger = self.components.merger.build(&context)?;
        merger.spawn(executor.clone())?;

        // Broadcaster component (transaction submission)
        let broadcaster = self.components.broadcaster.build(&context)?;
        broadcaster.spawn(executor.clone())?;

        // Health monitoring
        let health = self.components.health.build(&context)?;
        health.spawn(executor.clone())?;

        // Monitoring (metrics)
        let monitoring = self.components.monitoring.build(&context)?;
        monitoring.spawn(executor.clone())?;

        Ok(KabuHandle { _phantom: PhantomData })
    }
}

/// Handle to a running Kabu node
pub struct KabuHandle {
    _phantom: PhantomData<()>,
}

impl KabuHandle {
    /// Wait for the node to finish (never returns in normal operation)
    pub async fn wait_for_shutdown(self) -> eyre::Result<()> {
        // In a real implementation, this would wait on shutdown signals
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}
