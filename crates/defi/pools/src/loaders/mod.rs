mod curve;
mod maverick;
mod uniswap2;
mod uniswap3;

use crate::loaders::curve::CurvePoolLoader;
use alloy::providers::network::Ethereum;
use alloy::providers::{Network, Provider};
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEVM, KabuDataTypesEthereum};
use kabu_types_market::PoolClass;
use kabu_types_market::{PoolLoader, PoolLoaders, PoolsLoadingConfig};
pub use maverick::MaverickPoolLoader;
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

        pub struct $name<P, N, LDT = KabuDataTypesEthereum>
        where
            N: Network,
            P: Provider<N> + Clone,
            LDT: KabuDataTypes,
        {
            provider: Option<P>,
            phantom_data: PhantomData<(P, N, LDT)>,
        }

        #[allow(dead_code)]
        impl<P, N, LDT> $name<P, N, LDT>
        where
            N: Network,
            P: Provider<N> + Clone,
            LDT: KabuDataTypes,
        {
            pub fn new() -> Self {
                Self::default()
            }

            pub fn with_provider(provder: P) -> Self {
                Self { provider: Some(provder), phantom_data: PhantomData }
            }
        }

        impl<P, N, LDT> Default for $name<P, N, LDT>
        where
            N: Network,
            P: Provider<N> + Clone,
            LDT: KabuDataTypes,
        {
            fn default() -> Self {
                Self { provider: None, phantom_data: PhantomData }
            }
        }
    };
}

pub struct PoolLoadersBuilder<P, N = Ethereum, LDT = KabuDataTypesEthereum>
where
    N: Network,
    P: Provider<N> + 'static,
    LDT: KabuDataTypes,
{
    inner: PoolLoaders<P, N, LDT>,
}

impl<P, N, LDT> PoolLoadersBuilder<P, N, LDT>
where
    N: Network,
    P: Provider<N> + 'static,
    LDT: KabuDataTypes,
{
    pub fn new() -> PoolLoadersBuilder<P, N, LDT> {
        PoolLoadersBuilder { inner: PoolLoaders::<P, N, LDT>::new() }
    }

    pub fn with_provider<NP: Provider<N>>(self, provider: NP) -> PoolLoadersBuilder<NP, N, LDT> {
        PoolLoadersBuilder { inner: self.inner.with_provider(provider) }
    }

    pub fn with_config(self, config: PoolsLoadingConfig) -> Self {
        Self { inner: self.inner.with_config(config) }
    }

    pub fn add_loader<L: PoolLoader<P, N, LDT> + Send + Sync + Clone + 'static>(self, pool_class: PoolClass, pool_loader: L) -> Self {
        Self { inner: self.inner.add_loader(pool_class, pool_loader) }
    }

    pub fn build(self) -> PoolLoaders<P, N, LDT> {
        self.inner
    }
}

impl<P, N, LDT> Default for PoolLoadersBuilder<P, N, LDT>
where
    N: Network,
    P: Provider<N> + 'static,
    LDT: KabuDataTypes,
{
    fn default() -> Self {
        Self { inner: PoolLoaders::new() }
    }
}

impl<P, N, LDT> PoolLoadersBuilder<P, N, LDT>
where
    N: Network,
    P: Provider<N> + Clone + 'static,
    LDT: KabuDataTypesEVM + 'static,
{
    pub fn default_pool_loaders(provider: P, config: PoolsLoadingConfig) -> PoolLoaders<P, N, LDT>
    where
        P: Provider<N> + Clone,
    {
        PoolLoadersBuilder::<P, N, LDT>::new()
            .with_provider(provider.clone())
            .with_config(config)
            .add_loader(PoolClass::Maverick, MaverickPoolLoader::with_provider(provider.clone()))
            .add_loader(PoolClass::UniswapV2, UniswapV2PoolLoader::with_provider(provider.clone()))
            .add_loader(PoolClass::UniswapV3, UniswapV3PoolLoader::with_provider(provider.clone()))
            .add_loader(PoolClass::Curve, CurvePoolLoader::with_provider(provider.clone()))
            .build()
    }
}
