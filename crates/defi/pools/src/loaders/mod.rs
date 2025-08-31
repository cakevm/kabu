mod curve;
mod maverick;
mod uniswap2;
mod uniswap3;

use crate::loaders::curve::CurvePoolLoader;
use alloy::providers::network::Ethereum;
use alloy::providers::{Network, Provider};
use kabu_types_market::PoolClass;
use kabu_types_market::{PoolLoader, PoolLoaders, PoolsLoadingConfig};
pub use maverick::MaverickPoolLoader;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
pub use uniswap2::UniswapV2PoolLoader;
pub use uniswap3::UniswapV3PoolLoader;

/// creates  pool loader and imports necessary crates
#[macro_export]
macro_rules! pool_loader {
    // This will match the input like MaverickPoolLoader
    ($name:ident) => {
        use alloy::providers::{Network, Provider};
        use std::marker::PhantomData;

        #[derive(Clone)]

        pub struct $name<P, N, NP = EthPrimitives>
        where
            N: Network,
            P: Provider<N> + Clone,
            NP: Send + Sync,
        {
            provider: Option<P>,
            phantom_data: PhantomData<(P, N, NP)>,
        }

        #[allow(dead_code)]
        impl<P, N, NP> $name<P, N, NP>
        where
            N: Network,
            P: Provider<N> + Clone,
            NP: Send + Sync,
        {
            pub fn new() -> Self {
                Self::default()
            }

            pub fn with_provider(provder: P) -> Self {
                Self { provider: Some(provder), phantom_data: PhantomData }
            }
        }

        impl<P, N, NP> Default for $name<P, N, NP>
        where
            N: Network,
            P: Provider<N> + Clone,
            NP: Send + Sync,
        {
            fn default() -> Self {
                Self { provider: None, phantom_data: PhantomData }
            }
        }
    };
}

pub struct PoolLoadersBuilder<P, N = Ethereum, NP = EthPrimitives>
where
    N: Network,
    P: Provider<N> + 'static,
    NP: NodePrimitives,
{
    inner: PoolLoaders<P, N, NP>,
}

impl<P, N, NP> PoolLoadersBuilder<P, N, NP>
where
    N: Network,
    P: Provider<N> + 'static,
    NP: NodePrimitives,
{
    pub fn new() -> PoolLoadersBuilder<P, N, NP> {
        PoolLoadersBuilder { inner: PoolLoaders::<P, N, NP>::new() }
    }

    pub fn with_provider<NewP: Provider<N>>(self, provider: NewP) -> PoolLoadersBuilder<NewP, N, NP> {
        PoolLoadersBuilder { inner: self.inner.with_provider(provider) }
    }

    pub fn with_config(self, config: PoolsLoadingConfig) -> Self {
        Self { inner: self.inner.with_config(config) }
    }

    pub fn add_loader<L: PoolLoader<P, N, NP> + Send + Sync + Clone + 'static>(self, pool_class: PoolClass, pool_loader: L) -> Self {
        Self { inner: self.inner.add_loader(pool_class, pool_loader) }
    }

    pub fn build(self) -> PoolLoaders<P, N, NP> {
        self.inner
    }
}

impl<P, N, NP> Default for PoolLoadersBuilder<P, N, NP>
where
    N: Network,
    P: Provider<N> + 'static,
    NP: NodePrimitives,
{
    fn default() -> Self {
        Self { inner: PoolLoaders::new() }
    }
}

impl<P, N, NP> PoolLoadersBuilder<P, N, NP>
where
    N: Network,
    P: Provider<N> + Clone + 'static,
    NP: NodePrimitives + 'static,
{
    pub fn default_pool_loaders(provider: P, config: PoolsLoadingConfig) -> PoolLoaders<P, N, NP>
    where
        P: Provider<N> + Clone,
    {
        PoolLoadersBuilder::<P, N, NP>::new()
            .with_provider(provider.clone())
            .with_config(config)
            .add_loader(PoolClass::Maverick, MaverickPoolLoader::with_provider(provider.clone()))
            .add_loader(PoolClass::UniswapV2, UniswapV2PoolLoader::with_provider(provider.clone()))
            .add_loader(PoolClass::UniswapV3, UniswapV3PoolLoader::with_provider(provider.clone()))
            .add_loader(PoolClass::Curve, CurvePoolLoader::with_provider(provider.clone()))
            .build()
    }
}
