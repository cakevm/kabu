use std::collections::BTreeMap;

use alloy_primitives::U256;
use kabu_core_actors::SharedState;
use kabu_types_blockchain::GethStateUpdateVec;
use kabu_types_entities::{EntityAddress, Market, PoolWrapper, SwapDirection};
use tracing::debug;

pub async fn get_affected_pools_from_state_update(
    market: SharedState<Market>,
    state_update: &GethStateUpdateVec,
) -> BTreeMap<PoolWrapper, Vec<SwapDirection>> {
    let market_guard = market.read().await;

    let mut affected_pools: BTreeMap<PoolWrapper, Vec<SwapDirection>> = BTreeMap::new();

    for state_update_record in state_update.iter() {
        for (address, state_update_entry) in state_update_record.iter() {
            if market_guard.is_pool_manager(address) {
                for cell in state_update_entry.storage.keys() {
                    let cell_u: U256 = U256::from_be_slice(cell.as_slice());
                    if let Some(pool_id) = market_guard.get_pool_id_for_cell(address, &cell_u) {
                        if let Some(pool) = market_guard.get_pool(pool_id) {
                            if !affected_pools.contains_key(pool) {
                                debug!("Affected pool_managers {} pool {} ", address, pool_id);
                                affected_pools.insert(pool.clone(), pool.get_swap_directions());
                            }
                        }
                    }
                }
            } else if let Some(pool) = market_guard.get_pool(&EntityAddress::Address(*address)) {
                if !affected_pools.contains_key(pool) {
                    affected_pools.insert(pool.clone(), pool.get_swap_directions());
                }
            }
        }
    }

    affected_pools
}
