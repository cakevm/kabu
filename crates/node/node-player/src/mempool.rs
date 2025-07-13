use alloy_rpc_types::Header;
use eyre::Result;
use kabu_core_actors::SharedState;
use kabu_evm_db::KabuDBError;
use kabu_evm_utils::evm_env::header_to_block_env;
use kabu_types_blockchain::Mempool;
use kabu_types_entities::MarketState;
use revm::{Database, DatabaseCommit, DatabaseRef};
use tracing::debug;

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
        let _market_state_guard = market_state.write().await;

        for (_tx_hash, mempool_tx) in mempool_guard.txs.iter_mut() {
            if mempool_tx.mined == Some(header.number) {
                let _block_env = header_to_block_env(&header);

                // TODO: Fix me
                /*
                let result_and_state = evm_call(&market_state_guard.state_db, mempool_tx.tx.clone().unwrap())?;
                let (logs, state_update) = convert_evm_result_to_rpc(
                    result_and_state,
                    *tx_hash,
                    BlockNumHash { number: header.number, hash: header.hash },
                    header.timestamp,
                )?;
                info!("Updating state for mempool tx {} logs: {} state_updates : {}", tx_hash, logs.len(), state_update.len());

                mempool_tx.logs = Some(logs);
                mempool_tx.state_update = Some(state_update);

                 */
            }
        }
    } else {
        debug!("Mempool is empty");
    }
    Ok(())
}
