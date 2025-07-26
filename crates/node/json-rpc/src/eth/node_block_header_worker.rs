use std::collections::HashMap;

use alloy_network::Network;
use alloy_primitives::BlockHash;
use alloy_provider::Provider;
use alloy_rpc_types::Header;
use chrono::Utc;
use futures::StreamExt;
use kabu_core_actors::{Broadcaster, WorkerResult};
use kabu_types_blockchain::KabuDataTypes;
use kabu_types_events::{BlockHeaderEventData, MessageBlockHeader};
use tracing::{error, info};

pub async fn new_node_block_header_worker<P, N, LDT>(
    client: P,
    new_block_header_channel: Broadcaster<Header>,
    block_header_channel: Broadcaster<MessageBlockHeader<LDT>>,
) -> WorkerResult
where
    N: Network<HeaderResponse = Header>,
    P: Provider<N> + Send + Sync + Clone + 'static,
    LDT: KabuDataTypes,
{
    info!("Starting node block header worker");
    let sub = client.subscribe_blocks().await?;
    let mut stream = sub.into_stream();

    let mut block_processed: HashMap<BlockHash, chrono::DateTime<Utc>> = HashMap::new();

    loop {
        tokio::select! {
            block_msg = stream.next() => {
                if let Some(block_header) = block_msg {
                    let block_hash = block_header.hash_slow();
                    info!("Block hash received: {:?}" , block_hash);
                    if let std::collections::hash_map::Entry::Vacant(e) = block_processed.entry(block_hash) {
                        e.insert(Utc::now());
                        if let Err(e) =  new_block_header_channel.send(block_header.clone()) {
                            error!("Block hash broadcaster error  {}", e);
                        }
                        if let Err(e) = block_header_channel.send(MessageBlockHeader::new_with_time(BlockHeaderEventData::<LDT>::new(block_header))) {
                            error!("Block header broadcaster error {}", e);
                        }
                    }
                }
            }
        }
    }
}
