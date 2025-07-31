use crate::{BuilderContext, Component};
use eyre::Result;
use std::future::Future;

/// Builder for pool/mempool components
pub trait PoolBuilder<State>: Send {
    /// The pool component type
    type Pool: Component;

    /// Build the pool component
    fn build_pool(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Pool>> + Send;
}

/// Builder for network components
pub trait NetworkBuilder<State>: Send {
    /// The network component type
    type Network: Component;

    /// Build the network component
    fn build_network(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Network>> + Send;
}

/// Builder for executor components
pub trait ExecutorBuilder<State>: Send {
    /// The executor component type
    type Executor: Component;

    /// Build the executor component
    fn build_executor(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Executor>> + Send;
}

/// Builder for strategy components (MEV strategies like arbitrage, liquidation)
pub trait StrategyBuilder<State>: Send {
    /// The strategy component type
    type Strategy: Component;

    /// Build the strategy component
    fn build_strategy(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Strategy>> + Send;
}

/// Builder for transaction signing components
pub trait SignerBuilder<State>: Send {
    /// The signer component type
    type Signer: Component;

    /// Build the signer component
    fn build_signer(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Signer>> + Send;
}

/// Builder for market data components (pools, tokens, prices)
pub trait MarketBuilder<State>: Send {
    /// The market component type
    type Market: Component;

    /// Build the market component
    fn build_market(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Market>> + Send;
}

/// Builder for broadcaster components (flashbots, mempool, etc.)
pub trait BroadcasterBuilder<State>: Send {
    /// The broadcaster component type
    type Broadcaster: Component;

    /// Build the broadcaster component
    fn build_broadcaster(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Broadcaster>> + Send;
}

/// Builder for estimator components (gas, profit estimation)
pub trait EstimatorBuilder<State>: Send {
    /// The estimator component type
    type Estimator: Component;

    /// Build the estimator component
    fn build_estimator(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Estimator>> + Send;
}

/// Builder for health monitoring components
pub trait HealthMonitorBuilder<State>: Send {
    /// The health monitor component type
    type HealthMonitor: Component;

    /// Build the health monitor component
    fn build_health_monitor(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::HealthMonitor>> + Send;
}

/// Builder for consensus components (for blockchain compatibility)
pub trait ConsensusBuilder<State>: Send {
    /// The consensus component type
    type Consensus: Component;

    /// Build the consensus component
    fn build_consensus(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Consensus>> + Send;
}

/// Builder for payload/block building components
pub trait PayloadBuilder<State>: Send {
    /// The payload builder component type
    type PayloadBuilder: Component;

    /// Build the payload builder component
    fn build_payload(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::PayloadBuilder>> + Send;
}

/// Combined builder trait for all MEV node components
pub trait MevNodeComponentsBuilder<State>: Send {
    /// Pool component builder
    type Pool: PoolBuilder<State>;
    /// Network component builder
    type Network: NetworkBuilder<State>;
    /// Executor component builder
    type Executor: ExecutorBuilder<State>;
    /// Strategy component builder
    type Strategy: StrategyBuilder<State>;
    /// Signer component builder
    type Signer: SignerBuilder<State>;
    /// Market component builder
    type Market: MarketBuilder<State>;
    /// Broadcaster component builder
    type Broadcaster: BroadcasterBuilder<State>;
    /// Estimator component builder
    type Estimator: EstimatorBuilder<State>;
    /// Health monitor component builder
    type HealthMonitor: HealthMonitorBuilder<State>;

    /// Get the pool builder
    fn pool_builder(&self) -> Self::Pool;
    /// Get the network builder
    fn network_builder(&self) -> Self::Network;
    /// Get the executor builder
    fn executor_builder(&self) -> Self::Executor;
    /// Get the strategy builder
    fn strategy_builder(&self) -> Self::Strategy;
    /// Get the signer builder
    fn signer_builder(&self) -> Self::Signer;
    /// Get the market builder
    fn market_builder(&self) -> Self::Market;
    /// Get the broadcaster builder
    fn broadcaster_builder(&self) -> Self::Broadcaster;
    /// Get the estimator builder
    fn estimator_builder(&self) -> Self::Estimator;
    /// Get the health monitor builder
    fn health_monitor_builder(&self) -> Self::HealthMonitor;
}

/// Builder for initialization components (one-shot actors that set up initial state)
pub trait InitializerBuilder<State>: Send {
    /// The initializer component type
    type Initializer: Component;

    /// Build the initializer component
    fn build_initializer(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Initializer>> + Send;
}

/// Builder for merger components (combines multiple strategies or paths)
pub trait MergerBuilder<State>: Send {
    /// The merger component type
    type Merger: Component;

    /// Build the merger component
    fn build_merger(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Merger>> + Send;
}

/// Builder for web server components
pub trait WebServerBuilder<State>: Send {
    /// The web server component type
    type WebServer: Component;

    /// Build the web server component
    fn build_web_server(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::WebServer>> + Send;
}

/// Builder for monitoring components (influxdb, metrics)
pub trait MonitoringBuilder<State>: Send {
    /// The monitoring component type
    type Monitoring: Component;

    /// Build the monitoring component
    fn build_monitoring(self, ctx: &BuilderContext<State>) -> impl Future<Output = Result<Self::Monitoring>> + Send;
}

/// Combined builder trait for basic node components (for simpler nodes)
pub trait NodeComponentsBuilder<State>: Send {
    /// Pool component builder
    type Pool: PoolBuilder<State>;
    /// Network component builder
    type Network: NetworkBuilder<State>;
    /// Executor component builder
    type Executor: ExecutorBuilder<State>;
    /// Strategy component builder
    type Strategy: StrategyBuilder<State>;

    /// Get the pool builder
    fn pool_builder(&self) -> Self::Pool;
    /// Get the network builder
    fn network_builder(&self) -> Self::Network;
    /// Get the executor builder
    fn executor_builder(&self) -> Self::Executor;
    /// Get the strategy builder
    fn strategy_builder(&self) -> Self::Strategy;
}
