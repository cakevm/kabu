use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use alloy_rpc_types::Log;
use alloy_sol_types::SolEventInterface;
use eyre::Result;

use kabu_defi_abi::uniswap4::IUniswapV4PoolManagerEvents::IUniswapV4PoolManagerEventsEvents;
use kabu_defi_address_book::FactoryAddress;
use kabu_types_market::SwapDirection;
use kabu_types_market::{Market, PoolId, PoolWrapper};

#[allow(dead_code)]
pub async fn get_affected_pools_from_logs(market: Arc<RwLock<Market>>, logs: &[Log]) -> Result<BTreeMap<PoolWrapper, Vec<SwapDirection>>> {
    let market_guard = market.read().await;

    let mut affected_pools: BTreeMap<PoolWrapper, Vec<SwapDirection>> = BTreeMap::new();

    for log in logs.iter() {
        if log.address().eq(&FactoryAddress::UNISWAP_V4_POOL_MANAGER_ADDRESS) {
            if let Some(pool_id) = match IUniswapV4PoolManagerEventsEvents::decode_log(&log.inner) {
                Ok(event) => match event.data {
                    IUniswapV4PoolManagerEventsEvents::Initialize(params) => Some(params.id),
                    IUniswapV4PoolManagerEventsEvents::ModifyLiquidity(params) => Some(params.id),
                    IUniswapV4PoolManagerEventsEvents::Swap(params) => Some(params.id),
                    IUniswapV4PoolManagerEventsEvents::Donate(params) => Some(params.id),
                },
                Err(_) => None,
            } {
                if let Some(pool) = market_guard.get_pool(&PoolId::B256(pool_id)) {
                    if !affected_pools.contains_key(pool) {
                        affected_pools.insert(pool.clone(), pool.get_swap_directions());
                    }
                }
            }
        }
    }

    Ok(affected_pools)
}
