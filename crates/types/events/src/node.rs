use crate::Message;
use alloy_consensus::BlockHeader;
use alloy_primitives::TxHash;
use kabu_types_blockchain::{GethStateUpdateVec, MempoolTx};
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;

#[derive(Clone, Debug)]
pub struct NodeMempoolDataUpdate<N: NodePrimitives> {
    pub tx_hash: TxHash,
    pub mempool_tx: MempoolTx<N>,
}

#[derive(Clone, Debug)]
pub struct BlockUpdate<N: NodePrimitives = EthPrimitives> {
    pub block: N::Block,
}

#[derive(Clone, Debug)]
pub struct BlockStateUpdate<N: NodePrimitives = EthPrimitives> {
    pub block_header: N::BlockHeader,
    pub state_update: GethStateUpdateVec,
}

#[derive(Clone, Debug)]
pub struct BlockHeaderEventData<N: NodePrimitives = EthPrimitives> {
    pub header: N::BlockHeader,
    pub next_block_number: u64,
    pub next_block_timestamp: u64,
}

pub type MessageMempoolDataUpdate<N> = Message<NodeMempoolDataUpdate<N>>;

pub type MessageBlockHeader<N = EthPrimitives> = Message<BlockHeaderEventData<N>>;
pub type MessageBlock<N = EthPrimitives> = Message<BlockUpdate<N>>;
pub type MessageBlockStateUpdate<N = EthPrimitives> = Message<BlockStateUpdate<N>>;

impl<N: NodePrimitives> BlockHeaderEventData<N>
where
    N::BlockHeader: BlockHeader,
{
    pub fn new(header: N::BlockHeader) -> Self {
        let next_block_number = header.number() + 1;
        let next_block_timestamp = header.timestamp() + 12;
        Self { header, next_block_number, next_block_timestamp }
    }
}
