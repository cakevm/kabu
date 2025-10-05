//! Node types and component set builders

use crate::defaults::{
    DefaultBroadcaster, DefaultEstimator, DefaultExecutor, DefaultHealthMonitor, DefaultMarket, DefaultMerger, DefaultMonitoring,
    DefaultNetwork, DefaultSigner, DefaultStrategy,
};
use crate::traits::DisabledComponent;

/// Marker type for the Kabu Ethereum node
pub struct KabuEthereumNode;

/// A set of components that make up a complete node
pub struct ComponentSet<N, E, S, M, B, Est, H, Mer, Mon, Sig> {
    pub network: N,
    pub executor: E,
    pub strategy: S,
    pub market: M,
    pub broadcaster: B,
    pub estimator: Est,
    pub health: H,
    pub merger: Mer,
    pub monitoring: Mon,
    pub signer: Sig,
}

impl KabuEthereumNode {
    /// Start building components with defaults
    pub fn components() -> ComponentSetBuilder<
        DefaultNetwork,
        DefaultExecutor,
        DefaultStrategy,
        DefaultMarket,
        DefaultBroadcaster,
        DefaultEstimator,
        DefaultHealthMonitor,
        DefaultMerger,
        DefaultMonitoring,
        DefaultSigner,
    > {
        ComponentSetBuilder {
            network: DefaultNetwork,
            executor: DefaultExecutor,
            strategy: DefaultStrategy,
            market: DefaultMarket,
            broadcaster: DefaultBroadcaster,
            estimator: DefaultEstimator,
            health: DefaultHealthMonitor,
            merger: DefaultMerger,
            monitoring: DefaultMonitoring,
            signer: DefaultSigner,
        }
    }
}

/// Builder for creating a component set with fluent API
pub struct ComponentSetBuilder<N, E, S, M, B, Est, H, Mer, Mon, Sig> {
    network: N,
    executor: E,
    strategy: S,
    market: M,
    broadcaster: B,
    estimator: Est,
    health: H,
    merger: Mer,
    monitoring: Mon,
    signer: Sig,
}

impl Default
    for ComponentSetBuilder<
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
        DisabledComponent,
    >
{
    fn default() -> Self {
        Self {
            network: DisabledComponent,
            executor: DisabledComponent,
            strategy: DisabledComponent,
            market: DisabledComponent,
            broadcaster: DisabledComponent,
            estimator: DisabledComponent,
            health: DisabledComponent,
            merger: DisabledComponent,
            monitoring: DisabledComponent,
            signer: DisabledComponent,
        }
    }
}

impl<N, E, S, M, B, Est, H, Mer, Mon, Sig> ComponentSetBuilder<N, E, S, M, B, Est, H, Mer, Mon, Sig> {
    /// Replace the network component
    pub fn network<NewN>(self, network: NewN) -> ComponentSetBuilder<NewN, E, S, M, B, Est, H, Mer, Mon, Sig> {
        ComponentSetBuilder {
            network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the executor component
    pub fn executor<NewE>(self, executor: NewE) -> ComponentSetBuilder<N, NewE, S, M, B, Est, H, Mer, Mon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the strategy component
    pub fn strategy<NewS>(self, strategy: NewS) -> ComponentSetBuilder<N, E, NewS, M, B, Est, H, Mer, Mon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the market component
    pub fn market<NewM>(self, market: NewM) -> ComponentSetBuilder<N, E, S, NewM, B, Est, H, Mer, Mon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the broadcaster component
    pub fn broadcaster<NewB>(self, broadcaster: NewB) -> ComponentSetBuilder<N, E, S, M, NewB, Est, H, Mer, Mon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the estimator component
    pub fn estimator<NewEst>(self, estimator: NewEst) -> ComponentSetBuilder<N, E, S, M, B, NewEst, H, Mer, Mon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the health monitor component
    pub fn health<NewH>(self, health: NewH) -> ComponentSetBuilder<N, E, S, M, B, Est, NewH, Mer, Mon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the merger component
    pub fn merger<NewMer>(self, merger: NewMer) -> ComponentSetBuilder<N, E, S, M, B, Est, H, NewMer, Mon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }

    /// Replace the monitoring component
    pub fn monitoring<NewMon>(self, monitoring: NewMon) -> ComponentSetBuilder<N, E, S, M, B, Est, H, Mer, NewMon, Sig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring,
            signer: self.signer,
        }
    }

    /// Replace the signer component
    pub fn signer<NewSig>(self, signer: NewSig) -> ComponentSetBuilder<N, E, S, M, B, Est, H, Mer, Mon, NewSig> {
        ComponentSetBuilder {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer,
        }
    }

    /// Build the final component set
    pub fn build(self) -> ComponentSet<N, E, S, M, B, Est, H, Mer, Mon, Sig> {
        ComponentSet {
            network: self.network,
            executor: self.executor,
            strategy: self.strategy,
            market: self.market,
            broadcaster: self.broadcaster,
            estimator: self.estimator,
            health: self.health,
            merger: self.merger,
            monitoring: self.monitoring,
            signer: self.signer,
        }
    }
}
