use alloy_json_rpc::RpcRecv;
use alloy_network::{BlockResponse, Network};
use alloy_provider::Provider;
use alloy_rpc_types::Header;
use kabu_core_actors::{subscribe, Broadcaster, WorkerResult};
use kabu_types_blockchain::KabuDataTypesEVM;
use kabu_types_events::{BlockUpdate, Message, MessageBlock};
use tracing::{debug, error};

pub async fn new_block_with_tx_worker<P, N, LDT>(
    client: P,
    block_header_receiver: Broadcaster<Header>,
    sender: Broadcaster<MessageBlock<LDT>>,
) -> WorkerResult
where
    N: Network<BlockResponse = LDT::Block>,
    P: Provider<N> + Send + Sync + 'static,
    LDT: KabuDataTypesEVM,
    LDT::Block: RpcRecv + BlockResponse,
{
    subscribe!(block_header_receiver);

    loop {
        if let Ok(block_header) = block_header_receiver.recv().await {
            let (block_number, block_hash) = (block_header.inner.number, block_header.hash);
            debug!("BlockWithTx header received {} {}", block_number, block_hash);

            let mut err_counter = 0;

            while err_counter < 3 {
                match client.get_block_by_hash(block_hash).full().await {
                    Ok(block_with_tx) => {
                        if let Some(block_with_txes) = block_with_tx {
                            if let Err(e) = sender.send(Message::new_with_time(BlockUpdate { block: block_with_txes })) {
                                error!("Broadcaster error {}", e);
                            }
                        } else {
                            error!("BlockWithTx is empty");
                        }
                        break;
                    }
                    Err(e) => {
                        error!("client.get_block_by_hash {e}");
                        err_counter += 1;
                    }
                }
            }

            debug!("BlockWithTx processing finished {} {}", block_number, block_hash);
        }
    }
}
