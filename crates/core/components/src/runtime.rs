use crate::{BuilderContext, ComponentsBuilder};
use alloy_network::Ethereum;
use alloy_provider::Provider;
use eyre::Result;
use kabu_core_blockchain::{AppState, BlockchainState, EventChannels, Strategy};
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_entities::TxSigners;
use reth_tasks::TaskExecutor;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Runtime state that components can access
pub struct RuntimeState<P, DB: Clone + Send + Sync + 'static, E, LDT: KabuDataTypes + 'static = KabuDataTypesEthereum> {
    pub provider: P,
    pub app_state: AppState<LDT>,
    pub blockchain_state: BlockchainState<DB, LDT>,
    pub channels: EventChannels<LDT>,
    pub strategy: Strategy<DB, LDT>,
    pub signers: Arc<RwLock<TxSigners<LDT>>>,
    pub encoder: E,
}

/// Main runtime for Kabu using the component architecture
pub struct KabuRuntime<P, N, DB: Clone + Send + Sync + 'static, E: Clone, LDT: KabuDataTypes + 'static = KabuDataTypesEthereum> {
    state: RuntimeState<P, DB, E, LDT>,
    executor: TaskExecutor,
    _n: PhantomData<N>,
}

impl<P, DB, E> KabuRuntime<P, Ethereum, DB, E, KabuDataTypesEthereum>
where
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: Send + Sync + Clone + 'static,
    E: Send + Sync + Clone + 'static,
{
    pub fn new(state: RuntimeState<P, DB, E, KabuDataTypesEthereum>, executor: TaskExecutor) -> Self {
        Self { state, executor, _n: PhantomData }
    }

    pub fn from_parts(
        provider: P,
        encoder: E,
        app_state: AppState<KabuDataTypesEthereum>,
        blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
        channels: EventChannels<KabuDataTypesEthereum>,
        strategy: Strategy<DB, KabuDataTypesEthereum>,
        executor: TaskExecutor,
    ) -> Self {
        let state = RuntimeState {
            provider,
            app_state,
            blockchain_state,
            channels,
            strategy,
            signers: Arc::new(RwLock::new(TxSigners::new())),
            encoder,
        };

        Self { state, executor, _n: PhantomData }
    }

    /// Get a components builder configured for this runtime
    pub fn components(&self) -> ComponentsBuilder<RuntimeState<P, DB, E, KabuDataTypesEthereum>> {
        ComponentsBuilder::default()
    }

    /// Build and spawn components
    pub async fn spawn_components<Pool, Network, Executor, Strategy>(
        &self,
        components: ComponentsBuilder<RuntimeState<P, DB, E, KabuDataTypesEthereum>, Pool, Network, Executor, Strategy>,
    ) -> Result<()>
    where
        Pool: crate::PoolBuilder<RuntimeState<P, DB, E, KabuDataTypesEthereum>>,
        Network: crate::NetworkBuilder<RuntimeState<P, DB, E, KabuDataTypesEthereum>>,
        Executor: crate::ExecutorBuilder<RuntimeState<P, DB, E, KabuDataTypesEthereum>>,
        Strategy: crate::StrategyBuilder<RuntimeState<P, DB, E, KabuDataTypesEthereum>>,
    {
        let ctx = BuilderContext::new(self.state.clone());
        components.build_and_spawn(ctx, self.executor.clone()).await
    }

    /// Access the runtime state
    pub fn state(&self) -> &RuntimeState<P, DB, E, KabuDataTypesEthereum> {
        &self.state
    }

    /// Access the task executor
    pub fn executor(&self) -> &TaskExecutor {
        &self.executor
    }
}

impl<P, DB, E, LDT: KabuDataTypes> Clone for RuntimeState<P, DB, E, LDT>
where
    P: Clone,
    DB: Clone + Send + Sync + 'static,
    E: Clone,
{
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            app_state: self.app_state.clone(),
            blockchain_state: self.blockchain_state.clone(),
            channels: self.channels.clone(),
            strategy: self.strategy.clone(),
            signers: self.signers.clone(),
            encoder: self.encoder.clone(),
        }
    }
}
