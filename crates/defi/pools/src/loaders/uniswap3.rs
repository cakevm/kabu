use crate::protocols::{UniswapV3Protocol, fetch_uni3_factory};
use crate::{MaverickPool, PancakeV3Pool, UniswapV3Pool, pool_loader};
use alloy::primitives::{Bytes, Log};
use alloy::sol_types::SolEventInterface;
use alloy_evm::EvmEnv;
use eyre::eyre;
use futures::Stream;
use kabu_defi_abi::uniswap3::IUniswapV3Pool::IUniswapV3PoolEvents;
use kabu_evm_db::KabuDBError;
use kabu_types_market::PoolClass;
use kabu_types_market::{PoolId, PoolLoader, PoolProtocol, PoolWrapper, get_protocol_by_factory};
use reth_ethereum_primitives::EthPrimitives;
use revm::DatabaseRef;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::error;

pool_loader!(UniswapV3PoolLoader);

impl<P, N, LDT> PoolLoader<P, N, LDT> for UniswapV3PoolLoader<P, N, LDT>
where
    N: Network,
    P: Provider<N> + Clone + 'static,
    LDT: Send + Sync + 'static,
{
    fn get_pool_class_by_log(&self, log_entry: &Log) -> Option<(PoolId, PoolClass)> {
        let log_entry: Option<Log> = Log::new(log_entry.address, log_entry.data.topics().to_vec(), log_entry.data.data.clone());
        match log_entry {
            Some(log_entry) => match IUniswapV3PoolEvents::decode_log(&log_entry) {
                Ok(event) => match event.data {
                    IUniswapV3PoolEvents::Swap(_)
                    | IUniswapV3PoolEvents::Mint(_)
                    | IUniswapV3PoolEvents::Burn(_)
                    | IUniswapV3PoolEvents::Initialize(_) => Some((PoolId::Address(log_entry.address), PoolClass::UniswapV3)),
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
                PoolId::B256(_) => return Err(eyre!("UniswapV3 pools only support Address-based pool IDs")),
            };

            let factory_address_result = fetch_uni3_factory(provider.clone(), pool_address).await;
            match factory_address_result {
                Ok(factory_address) => match get_protocol_by_factory(factory_address) {
                    PoolProtocol::PancakeV3 => {
                        Ok(PoolWrapper::new(Arc::new(PancakeV3Pool::fetch_pool_data(provider.clone(), pool_address).await?)))
                    }
                    PoolProtocol::Maverick => {
                        Ok(PoolWrapper::new(Arc::new(MaverickPool::fetch_pool_data(provider.clone(), pool_address).await?)))
                    }
                    _ => Ok(PoolWrapper::new(Arc::new(UniswapV3Pool::fetch_pool_data(provider.clone(), pool_address).await?))),
                },
                Err(e) => {
                    error!("Error fetching factory address at {:#20x}: {}", pool_address, e);
                    Err(eyre!("CANNOT_GET_FACTORY_ADDRESS"))
                }
            }
        })
    }

    fn fetch_pool_by_id_from_evm(&self, pool_id: PoolId, db: &dyn DatabaseRef<Error = KabuDBError>) -> eyre::Result<PoolWrapper> {
        let pool_address = match pool_id {
            PoolId::Address(addr) => addr,
            PoolId::B256(_) => return Err(eyre!("UniswapV3 pools only support Address-based pool IDs")),
        };
        Ok(PoolWrapper::new(Arc::new(UniswapV3Pool::fetch_pool_data_evm(db, &EvmEnv::default(), pool_address)?)))
    }

    fn is_code(&self, code: &Bytes) -> bool {
        UniswapV3Protocol::is_code(code)
    }

    fn protocol_loader(&self) -> eyre::Result<Pin<Box<dyn Stream<Item = (PoolId, PoolClass)> + Send>>> {
        Err(eyre!("NOT_IMPLEMENTED"))
    }
}
