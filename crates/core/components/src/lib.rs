mod builder;
mod composite;
mod dependencies;
mod kabu_node;
mod mev_components;
mod node;
mod runtime;
mod traits;

pub use builder::{BuilderContext, Components, ComponentsBuilder, MevComponentChannels, MevComponents, MevComponentsBuilder};
pub use composite::{ComponentGroup, CompositeComponent};
pub use dependencies::{BuilderContextWithDeps, Dependencies};
pub use kabu_node::PlaceholderComponent;
pub use mev_components::{
    AccountMonitorBuilder, BlockMonitorBuilder, HealthMonitorBuilder, MevBotComponentsBuilder, PoolLoaderBuilder, PriceMonitorBuilder,
    SignerBuilder, StrategyRunnerBuilder,
};
pub use node::{DefaultNodeComponents, Node, NodeComponents};
pub use runtime::{KabuRuntime, RuntimeState};
pub use traits::{
    BroadcasterBuilder, ConsensusBuilder, EstimatorBuilder, ExecutorBuilder, HealthMonitorBuilder as HealthMonitorBuilderTrait,
    MarketBuilder, MevNodeComponentsBuilder, NetworkBuilder, NodeComponentsBuilder, PayloadBuilder, PoolBuilder,
    SignerBuilder as SignerBuilderTrait, StrategyBuilder,
};

use eyre::Result;
use reth_tasks::TaskExecutor;

/// A component is a long-running task that processes data
pub trait Component: Send + 'static {
    /// Spawn the component's tasks using the provided executor
    fn spawn(self, executor: TaskExecutor) -> Result<()>
    where
        Self: Sized;

    /// Spawn the component when it's boxed (for dynamic dispatch)
    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()>;

    /// Name of the component for logging
    fn name(&self) -> &'static str;

    /// Check if component is ready to start (optional validation)
    fn check_readiness(&self) -> Result<()> {
        Ok(())
    }
}
