use alloy_eips::BlockNumHash;
use alloy_rpc_types::Header;
use eyre::Result;
use kabu_core_actors::SharedState;
use kabu_evm_db::KabuDBError;
use kabu_evm_utils::{evm_call_raw, EVMParserHelper, KabuEVMWrapper};
use kabu_types_blockchain::Mempool;
use kabu_types_entities::MarketState;
use revm::{Database, DatabaseCommit, DatabaseRef};
use tracing::{debug, info};

pub(crate) async fn replayer_mempool_task<DB>(
    mempool: SharedState<Mempool>,
    market_state: SharedState<MarketState<DB>>,
    header: Header,
) -> Result<()>
where
    DB: DatabaseRef<Error = KabuDBError> + DatabaseCommit + Database<Error = KabuDBError> + Send + Sync + Clone + 'static,
{
    let mut mempool_guard = mempool.write().await;
    debug!("process_mempool_task");

    if !mempool_guard.is_empty() {
        debug!("Mempool is not empty : {}", mempool_guard.len());
        let market_state_guard = market_state.write().await;

        for (tx_hash, mempool_tx) in mempool_guard.txs.iter_mut() {
            if mempool_tx.mined == Some(header.number) {
                let mut evm = KabuEVMWrapper::new(market_state_guard.state_db.clone()).with_header(&header);

                // for (tx_hash, mempool_tx) in mempool_guard.txs.iter_mut() {
                //     if mempool_tx.mined == Some(header.number) {
                //         let result_and_state = evm_call_tx_in_block(mempool_tx.tx.clone().unwrap(), &market_state_guard.state_db, &header)?;
                //         let (logs, state_update) = convert_evm_result_to_rpc(
                //             result_and_state,
                //             *tx_hash,
                //             BlockNumHash { number: header.number, hash: header.hash },
                //             header.timestamp,
                //         )?;
                //         info!("Updating state for mempool tx {} logs: {} state_updates : {}", tx_hash, logs.len(), state_update.len());
                //
                //         mempool_tx.logs = Some(logs);
                //         mempool_tx.state_update = Some(state_update);
                //     }
                // }

                let result_and_state = evm_call_raw(evm.get_mut(), mempool_tx.tx.clone().unwrap())?;
                let (logs, state_update) = EVMParserHelper::convert_evm_result_to_rpc(
                    result_and_state,
                    *tx_hash,
                    BlockNumHash { number: header.number, hash: header.hash },
                    header.timestamp,
                )?;
                info!("Updating state for mempool tx {} logs: {} state_updates : {}", tx_hash, logs.len(), state_update.len());

                mempool_tx.logs = Some(logs);
                mempool_tx.state_update = Some(state_update);
            }
        }
    } else {
        debug!("Mempool is empty");
    }
    Ok(())
}
