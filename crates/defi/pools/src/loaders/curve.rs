use crate::protocols::CurveProtocol;
use crate::{pool_loader, CurvePool};
use alloy::primitives::{Bytes, Log};
use async_stream::stream;
use eyre::eyre;
use futures::Stream;
use kabu_evm_db::KabuDBError;
use kabu_types_market::PoolClass;
use kabu_types_market::{PoolId, PoolLoader, PoolWrapper};
use reth_ethereum_primitives::EthPrimitives;
use revm::DatabaseRef;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::error;

pool_loader!(CurvePoolLoader);

impl<P, N, LDT> PoolLoader<P, N, LDT> for CurvePoolLoader<P, N, LDT>
where
    N: Network,
    P: Provider<N> + Clone + 'static,
    LDT: Send + Sync + 'static,
{
    fn get_pool_class_by_log(&self, _log_entry: &Log) -> Option<(PoolId, PoolClass)> {
        None
    }

    fn fetch_pool_by_id<'a>(&'a self, pool_id: PoolId) -> Pin<Box<dyn Future<Output = eyre::Result<PoolWrapper>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(provider) = &self.provider {
                self.fetch_pool_by_id_from_provider(pool_id, provider.clone()).await
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
                _ => return Err(eyre!("Pool ID must be an address for Curve pools")),
            };
            match CurveProtocol::get_contract_from_code(provider.clone(), pool_address).await {
                Ok(curve_contract) => {
                    let curve_pool = CurvePool::<P, N>::fetch_pool_data_with_default_encoder(provider.clone(), curve_contract).await?;

                    Ok(PoolWrapper::new(Arc::new(curve_pool)))
                }
                Err(e) => {
                    error!("Error getting curve contract from code {} : {} ", pool_address, e);
                    Err(e)
                }
            }
        })
    }

    fn fetch_pool_by_id_from_evm(&self, _pool_id: PoolId, _db: &dyn DatabaseRef<Error = KabuDBError>) -> eyre::Result<PoolWrapper> {
        Err(eyre!("NOT_IMPLEMENTED"))
    }

    fn is_code(&self, _code: &Bytes) -> bool {
        false
    }

    fn protocol_loader(&self) -> eyre::Result<Pin<Box<dyn Stream<Item = (PoolId, PoolClass)> + Send>>> {
        let provider_clone = self.provider.clone();

        if let Some(client) = provider_clone {
            Ok(Box::pin(stream! {
                let curve_contracts = CurveProtocol::get_contracts_vec(client.clone());
                for curve_contract in curve_contracts.iter() {
                    yield (PoolId::Address(curve_contract.get_address()), PoolClass::Curve)
                }

                for factory_idx in 0..10 {
                    if let Ok(factory_address) = CurveProtocol::get_factory_address(client.clone(), factory_idx).await {
                        if let Ok(pool_count) = CurveProtocol::get_pool_count(client.clone(), factory_address).await {
                            for pool_id in 0..pool_count {
                                if let Ok(addr) = CurveProtocol::get_pool_address(client.clone(), factory_address, pool_id).await {
                                    yield (PoolId::Address(addr), PoolClass::Curve)
                                }
                            }
                        }
                    }
                }
            }))
        } else {
            Err(eyre!("NO_PROVIDER"))
        }
    }
}
