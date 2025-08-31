use alloy_network::Network;
use alloy_primitives::Address;
use alloy_provider::Provider;
use eyre::Result;
use kabu_core_components::Component;
use reth_tasks::TaskExecutor;
use revm::DatabaseRef;
use revm::{Database, DatabaseCommit};
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::pool_loader_actor::fetch_and_add_pool_by_pool_id;

use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_evm_db::KabuDBError;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_market::required_state::{RequiredState, RequiredStateReader};
use kabu_types_market::MarketState;
use kabu_types_market::{Market, PoolClass, PoolId, PoolLoaders};
use reth_node_types::NodePrimitives;

async fn required_pools_loader_worker<P, N, DB, NP>(
    client: P,
    pool_loaders: Arc<PoolLoaders<P, N, NP>>,
    pools: Vec<(PoolId, PoolClass)>,
    required_state: Option<RequiredState>,
    market: Arc<RwLock<Market>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
) -> Result<String>
where
    N: Network<TransactionRequest = alloy_rpc_types::TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: Database<Error = KabuDBError> + DatabaseRef<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
    NP: NodePrimitives + 'static,
{
    for (pool_id, pool_class) in pools {
        debug!(class=%pool_class, %pool_id, "Loading pool");
        match fetch_and_add_pool_by_pool_id(client.clone(), market.clone(), market_state.clone(), pool_loaders.clone(), pool_id, pool_class)
            .await
        {
            Ok(_) => {
                info!(class=%pool_class, %pool_id, "pool loaded")
            }
            Err(error) => {
                error!(%error, "load_pool_with_provider")
            }
        }
    }
    //
    //
    //     match pool_class {
    //         PoolClass::UniswapV2 | PoolClass::UniswapV3 => {
    //             if let Err(error) =
    //                 fetch_and_add_pool_by_pool_id(client.clone(), market.clone(), market_state.clone(), pool_address, pool_class).await
    //             {
    //                 error!(%error, address = %pool_address, "fetch_and_add_pool_by_address")
    //             }
    //         }
    //         PoolClass::Curve => {
    //             if let Ok(curve_contract) = CurveProtocol::get_contract_from_code(client.clone(), pool_address).await {
    //                 let curve_pool = CurvePool::<P, T, N>::fetch_pool_data_with_default_encoder(client.clone(), curve_contract).await?;
    //                 fetch_state_and_add_pool(client.clone(), market.clone(), market_state.clone(), curve_pool.into()).await?
    //             } else {
    //                 error!("CURVE_POOL_NOT_LOADED");
    //             }
    //         }
    //         _ => {
    //             error!("Unknown pool class")
    //         }
    //     }
    //     debug!(class=%pool_class, address=%pool_address, "Loaded pool");
    // }

    if let Some(required_state) = required_state {
        let update = RequiredStateReader::fetch_calls_and_slots(client.clone(), required_state, None).await?;
        market_state.write().await.apply_geth_update(update);
    }

    Ok("required_pools_loader_worker".to_string())
}

pub struct RequiredPoolLoaderActor<P, N, DB, NP>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: Database + DatabaseRef + DatabaseCommit + Clone + Send + Sync + 'static,
    NP: NodePrimitives + 'static,
{
    client: P,
    pool_loaders: Arc<PoolLoaders<P, N, NP>>,
    pools: Vec<(PoolId, PoolClass)>,
    required_state: Option<RequiredState>,

    market: Option<Arc<RwLock<Market>>>,

    market_state: Option<Arc<RwLock<MarketState<DB>>>>,
    _n: PhantomData<N>,
}

impl<P, N, DB, NP> RequiredPoolLoaderActor<P, N, DB, NP>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: Database + DatabaseRef + DatabaseCommit + Clone + Send + Sync + 'static,
    NP: NodePrimitives + 'static,
{
    pub fn new(client: P, pool_loaders: Arc<PoolLoaders<P, N, NP>>) -> Self {
        Self { client, pools: Vec::new(), pool_loaders, required_state: None, market: None, market_state: None, _n: PhantomData }
    }

    pub fn with_pool_address(self, address: Address, pool_class: PoolClass) -> Self {
        let mut pools = self.pools;
        pools.push((PoolId::Address(address), pool_class));
        Self { pools, ..self }
    }

    pub fn on_bc(self, bc: &Blockchain<NP>, state: &BlockchainState<DB, NP>) -> Self {
        Self { market: Some(bc.market()), market_state: Some(state.market_state_commit()), ..self }
    }

    pub fn with_required_state(self, required_state: RequiredState) -> Self {
        Self { required_state: Some(required_state), ..self }
    }
}

impl<P, N, DB, NP> Component for RequiredPoolLoaderActor<P, N, DB, NP>
where
    N: Network<TransactionRequest = alloy_rpc_types::TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: Database<Error = KabuDBError> + DatabaseRef<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
    NP: NodePrimitives + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let _name = self.name();
        let client = self.client;
        let pool_loaders = self.pool_loaders;
        let pools = self.pools;
        let required_state = self.required_state;
        let market = self.market.ok_or_else(|| eyre::eyre!("market not set"))?;
        let market_state = self.market_state.ok_or_else(|| eyre::eyre!("market_state not set"))?;

        executor.spawn(async move {
            if let Err(e) = required_pools_loader_worker(client, pool_loaders, pools, required_state, market, market_state).await {
                error!("Required pools loader worker failed: {}", e);
            }
        });

        Ok(())
    }
    fn name(&self) -> &'static str {
        "RequiredPoolLoaderActor"
    }
}
