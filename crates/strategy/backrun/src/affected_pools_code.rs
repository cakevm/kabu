use alloy_eips::BlockNumberOrTag;
use alloy_network::Network;
use alloy_provider::Provider;
use eyre::eyre;
use std::collections::BTreeMap;
use tracing::{debug, error};

use kabu_core_actors::SharedState;
use kabu_defi_pools::protocols::{UniswapV2Protocol, UniswapV3Protocol};
use kabu_evm_db::{AlloyDB, KabuDB};
use kabu_types_blockchain::GethStateUpdateVec;
use kabu_types_entities::{EntityAddress, Market, MarketState, PoolWrapper, SwapDirection};

pub async fn get_affected_pools_from_code<P, N>(
    client: P,
    market: SharedState<Market>,
    state_update: &GethStateUpdateVec,
) -> eyre::Result<BTreeMap<PoolWrapper, Vec<SwapDirection>>>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    let mut market_state = MarketState::new(KabuDB::new());

    market_state.state_db.apply_geth_state_update(state_update, true, false);

    let ret: BTreeMap<PoolWrapper, Vec<SwapDirection>> = BTreeMap::new();

    for state_update_record in state_update.iter() {
        for (address, state_update_entry) in state_update_record.iter() {
            if let Some(code) = &state_update_entry.code {
                if UniswapV2Protocol::is_code(code) {
                    match market.read().await.get_pool(&EntityAddress::Address(*address)) {
                        None => {
                            debug!(?address, "Loading UniswapV2 class pool");

                            let ext_db = AlloyDB::new(client.clone(), BlockNumberOrTag::Latest.into());

                            let Some(ext_db) = ext_db else {
                                error!("Cannot create AlloyDB");
                                continue;
                            };

                            let _state_db = market_state.state_db.clone().with_ext_db(ext_db);

                            // TODO: Fix KabuEVMWrapper initialization and LoomExecuteEvm trait implementation
                            // let mut evm = KabuEVMWrapper::new();
                            // match UniswapV3EvmStateReader::factory(evm.get_mut(), *address) {
                            //     Ok(_factory_address) => match UniswapV2Pool::fetch_pool_data_evm(evm.get_mut(), *address) {
                            #[allow(clippy::redundant_closure_call)]
                            match (|| -> eyre::Result<()> {
                                // Placeholder for now - LoomExecuteEvm trait not properly implemented yet
                                Err(eyre!("LoomExecuteEvm trait not implemented"))
                            })() {
                                Ok(_) => match (|| -> eyre::Result<()> {
                                    Err(eyre!("Pool loading disabled - LoomExecuteEvm trait not implemented"))
                                })() {
                                    Ok(_) => {
                                        // Pool loading functionality disabled - LoomExecuteEvm trait not implemented
                                        debug!(?address, "UniswapV2 pool loading disabled - LoomExecuteEvm not implemented");
                                    }
                                    Err(err) => {
                                        debug!(?address, %err, "Pool loading disabled - LoomExecuteEvm trait not implemented");
                                    }
                                },
                                Err(err) => {
                                    debug!(?address, %err, "Pool loading disabled - LoomExecuteEvm trait not implemented")
                                }
                            }
                        }
                        Some(pool) => {
                            debug!(?address, protocol = ?pool.get_protocol(), "Pool already exists");
                        }
                    }
                }

                if UniswapV3Protocol::is_code(code) {
                    match market.read().await.get_pool(&EntityAddress::Address(*address)) {
                        None => {
                            debug!(%address, "Loading UniswapV3 class pool");

                            let ext_db = AlloyDB::new(client.clone(), BlockNumberOrTag::Latest.into());

                            let Some(ext_db) = ext_db else {
                                error!("Cannot create AlloyDB");
                                continue;
                            };

                            let _state_db = market_state.state_db.clone().with_ext_db(ext_db);

                            // TODO: Fix KabuEVMWrapper initialization and LoomExecuteEvm trait implementation
                            // let mut evm = KabuEVMWrapper::new();
                            // match UniswapV3EvmStateReader::factory(evm.get_mut(), *address) {
                            #[allow(clippy::redundant_closure_call)]
                            match (|| -> eyre::Result<()> {
                                // Placeholder for now - LoomExecuteEvm trait not properly implemented yet
                                Err(eyre!("LoomExecuteEvm trait not implemented"))
                            })() {
                                Ok(_) => {
                                    // Pool loading functionality disabled - LoomExecuteEvm trait not implemented
                                    debug!(?address, "UniswapV3 pool loading disabled - LoomExecuteEvm not implemented");
                                }
                                Err(err) => {
                                    debug!(?address, %err, "Pool loading disabled - LoomExecuteEvm trait not implemented")
                                }
                            }
                        }
                        Some(pool) => {
                            debug!(?address, protocol = ?pool.get_protocol(), "Pool already exists")
                        }
                    }
                }
            }
        }
    }
    if !ret.is_empty() {
        Ok(ret)
    } else {
        Err(eyre!("NO_POOLS_LOADED"))
    }
}

/// Check if the state update code contains code for a UniswapV2 pair or UniswapV3 pool by looking for method signatures.
pub fn is_pool_code(state_update: &GethStateUpdateVec) -> bool {
    for state_update_record in state_update.iter() {
        for (_address, state_update_entry) in state_update_record.iter() {
            if let Some(code) = &state_update_entry.code {
                if UniswapV3Protocol::is_code(code) {
                    return true;
                }
                if UniswapV2Protocol::is_code(code) {
                    return true;
                }
            }
        }
    }
    false
}
