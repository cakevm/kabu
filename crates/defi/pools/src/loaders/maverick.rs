use crate::{pool_loader, MaverickPool};
use alloy::primitives::{Bytes, Log};
use alloy::sol_types::SolEventInterface;
use alloy_evm::EvmEnv;
use eyre::{eyre, Result};
use kabu_defi_abi::maverick::IMaverickPool::IMaverickPoolEvents;
use kabu_evm_db::KabuDBError;
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_market::PoolClass;
use kabu_types_market::{PoolId, PoolLoader, PoolWrapper};
use revm::DatabaseRef;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;

pool_loader!(MaverickPoolLoader);

impl<P, N, LDT> PoolLoader<P, N, LDT> for MaverickPoolLoader<P, N, LDT>
where
    N: Network,
    P: Provider<N> + Clone + 'static,
    LDT: KabuDataTypes + 'static,
{
    fn get_pool_class_by_log(&self, log_entry: &Log) -> Option<(PoolId, PoolClass)> {
        let log_entry: Option<Log> = Log::new(log_entry.address, log_entry.data.topics().to_vec(), log_entry.data.data.clone());
        match log_entry {
            Some(log_entry) => match IMaverickPoolEvents::decode_log(&log_entry) {
                Ok(event) => match event.data {
                    IMaverickPoolEvents::Swap(_) | IMaverickPoolEvents::AddLiquidity(_) | IMaverickPoolEvents::RemoveLiquidity(_) => {
                        Some((PoolId::Address(log_entry.address), PoolClass::Maverick))
                    }
                    _ => None,
                },
                Err(_) => None,
            },
            None => None,
        }
    }

    fn fetch_pool_by_id<'a>(&'a self, pool_id: PoolId) -> Pin<Box<dyn Future<Output = Result<PoolWrapper>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(provider) = self.provider.clone() {
                self.fetch_pool_by_id_from_provider(pool_id, provider).await
            } else {
                Err(eyre!("NO_PROVIDER"))
            }
        })
    }

    fn fetch_pool_by_id_from_provider(&self, pool_id: PoolId, provider: P) -> Pin<Box<dyn Future<Output = Result<PoolWrapper>> + Send>> {
        Box::pin(async move {
            let address = match pool_id {
                PoolId::Address(addr) => addr,
                PoolId::B256(_) => return Err(eyre!("B256 pool ID variant not supported for Maverick pools")),
            };
            Ok(PoolWrapper::new(Arc::new(MaverickPool::fetch_pool_data(provider.clone(), address).await?)))
        })
    }

    fn fetch_pool_by_id_from_evm(&self, pool_id: PoolId, db: &dyn DatabaseRef<Error = KabuDBError>) -> Result<PoolWrapper> {
        let address = match pool_id {
            PoolId::Address(addr) => addr,
            PoolId::B256(_) => return Err(eyre!("B256 pool ID variant not supported for Maverick pools")),
        };
        Ok(PoolWrapper::new(Arc::new(MaverickPool::fetch_pool_data_evm(db, &EvmEnv::default(), address)?)))
    }

    fn is_code(&self, _code: &Bytes) -> bool {
        false
    }

    fn protocol_loader(&self) -> Result<Pin<Box<dyn Stream<Item = (PoolId, PoolClass)> + Send>>> {
        Err(eyre!("NOT_IMPLEMENTED"))
    }
}
