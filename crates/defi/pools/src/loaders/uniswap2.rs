use crate::protocols::{fetch_uni2_factory, UniswapV2Protocol};
use crate::{pool_loader, UniswapV2Pool};
use alloy::primitives::{Bytes, Log};
use alloy::sol_types::SolEventInterface;
use alloy_evm::EvmEnv;
use eyre::eyre;
use futures::Stream;
use kabu_defi_abi::uniswap2::IUniswapV2Pair::IUniswapV2PairEvents;
use kabu_evm_db::KabuDBError;
use kabu_types_market::PoolClass;
use kabu_types_market::{get_protocol_by_factory, PoolId, PoolLoader, PoolProtocol, PoolWrapper};
use reth_ethereum_primitives::EthPrimitives;
use revm::DatabaseRef;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pool_loader!(UniswapV2PoolLoader);

impl<P, N, LDT> PoolLoader<P, N, LDT> for UniswapV2PoolLoader<P, N, LDT>
where
    N: Network,
    P: Provider<N> + Clone + 'static,
    LDT: Send + Sync + 'static,
{
    fn get_pool_class_by_log(&self, log_entry: &Log) -> Option<(PoolId, PoolClass)> {
        let log_entry: Option<Log> = Log::new(log_entry.address, log_entry.data.topics().to_vec(), log_entry.data.data.clone());
        match log_entry {
            Some(log_entry) => match IUniswapV2PairEvents::decode_log(&log_entry) {
                Ok(event) => match event.data {
                    IUniswapV2PairEvents::Swap(_)
                    | IUniswapV2PairEvents::Mint(_)
                    | IUniswapV2PairEvents::Burn(_)
                    | IUniswapV2PairEvents::Sync(_) => Some((PoolId::Address(log_entry.address), PoolClass::UniswapV2)),
                    _ => None,
                },
                Err(_) => None,
            },
            None => None,
        }
    }

    fn fetch_pool_by_id<'a>(&'a self, pool_id: PoolId) -> Pin<Box<dyn Future<Output = eyre::Result<PoolWrapper>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(provider) = self.provider.clone() {
                self.fetch_pool_by_id_from_provider(pool_id, provider).await
            } else {
                Err(eyre!("NO_PROVIDER"))
            }
        })
    }

    fn fetch_pool_by_id_from_provider<'a>(
        &'a self,
        pool_id: PoolId,
        provider: P,
    ) -> Pin<Box<dyn Future<Output = eyre::Result<PoolWrapper>> + Send + 'a>> {
        Box::pin(async move {
            let pool_address = match pool_id {
                PoolId::Address(addr) => addr,
                PoolId::B256(_) => return Err(eyre!("UniswapV2 pools only support Address-based pool IDs")),
            };
            let factory_address = fetch_uni2_factory(provider.clone(), pool_address).await?;
            match get_protocol_by_factory(factory_address) {
                PoolProtocol::NomiswapStable
                | PoolProtocol::Miniswap
                | PoolProtocol::Integral
                | PoolProtocol::Safeswap
                | PoolProtocol::AntFarm => Err(eyre!("POOL_PROTOCOL_NOT_SUPPORTED")),
                _ => Ok(PoolWrapper::new(Arc::new(UniswapV2Pool::fetch_pool_data(provider, pool_address).await?))),
            }
        })
    }

    fn fetch_pool_by_id_from_evm(&self, pool_id: PoolId, db: &dyn DatabaseRef<Error = KabuDBError>) -> eyre::Result<PoolWrapper> {
        let pool_address = match pool_id {
            PoolId::Address(addr) => addr,
            PoolId::B256(_) => return Err(eyre!("UniswapV2 pools only support Address-based pool IDs")),
        };
        Ok(PoolWrapper::new(Arc::new(UniswapV2Pool::fetch_pool_data_evm(db, &EvmEnv::default(), pool_address)?)))
    }

    fn is_code(&self, code: &Bytes) -> bool {
        UniswapV2Protocol::is_code(code)
    }

    fn protocol_loader(&self) -> eyre::Result<Pin<Box<dyn Stream<Item = (PoolId, PoolClass)> + Send>>> {
        Err(eyre!("NOT_IMPLEMENTED"))
    }
}
