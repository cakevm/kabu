use kabu_core_blockchain::Blockchain;
use kabu_types_blockchain::MempoolTx;
use kabu_types_events::{MessageMempoolDataUpdate, NodeMempoolDataUpdate};
use reth_transaction_pool::{EthPooledTransaction, TransactionPool};
use tokio::select;
use tracing::{debug, error, info};

pub async fn mempool_worker<Pool>(mempool: Pool, bc: Blockchain) -> eyre::Result<()>
where
    Pool: TransactionPool<Transaction = EthPooledTransaction> + Clone + 'static,
{
    info!("Mempool worker started");
    let mut tx_listener = mempool.new_transactions_listener();

    let mempool_tx = bc.new_mempool_tx_channel();

    // EthTxBuilder removed in new version

    loop {
        select! {
            tx_notification = tx_listener.recv() => {
                if let Some(tx_notification) = tx_notification {
                    let tx_hash = *tx_notification.transaction.hash();
                    // TODO: Implement proper transaction conversion from EthPooledTransaction to alloy RPC Transaction
                    // For now, we're skipping the transaction details in the mempool update
                    // This allows the system to track transaction hashes without full transaction data
                    let update_msg: MessageMempoolDataUpdate = MessageMempoolDataUpdate::new_with_source(NodeMempoolDataUpdate { tx_hash, mempool_tx: MempoolTx { tx: None, ..MempoolTx::default() } }, "exex".to_string());
                    if let Err(e) =  mempool_tx.send(update_msg) {
                        error!(error=?e.to_string(), "mempool_tx.send");
                    }else{
                        debug!(hash = ?tx_notification.transaction.hash(), "Received pool tx");
                    }
                }
            }
        }
    }
}
