use kabu_evm_utils::reth_types::decode_into_transaction;
use kabu_types_blockchain::Mempool;
use kabu_types_events::{MessageTxCompose, RlpState, TxComposeMessageType};
use std::sync::Arc;
use tokio::select;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info};

pub(crate) async fn replayer_compose_worker(
    mempool: Arc<RwLock<Mempool>>,
    mut compose_channel_rx: broadcast::Receiver<MessageTxCompose>,
) -> eyre::Result<()> {
    loop {
        select! {
            msg = compose_channel_rx.recv() => {
                if let Ok(msg) = msg {
                    if let TxComposeMessageType::Broadcast(broadcast_msg) = msg.inner {
                        info!("Broadcast compose message received. {:?}", broadcast_msg.tx_bundle);
                        for tx in broadcast_msg.rlp_bundle.unwrap_or_default() {
                            match tx {
                                RlpState::Backrun( rlp_tx) | RlpState::Stuffing( rlp_tx)=>{
                                    match decode_into_transaction( &rlp_tx ) {
                                        Ok(new_tx)=>{
                                            mempool.write().await.add_tx(new_tx);
                                        }
                                        Err(e)=>{
                                            error!("decode_into_transaction {}", e);
                                        }
                                    }

                                }
                                _=>{
                                    error!("Unknown RLP tx type");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
