use crate::Message;
use kabu_types_blockchain::{GethStateUpdateVec, KabuHeader, MempoolTx};
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};

#[derive(Clone, Debug)]
pub struct NodeMempoolDataUpdate<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub tx_hash: LDT::TxHash,
    pub mempool_tx: MempoolTx<LDT>,
}

#[derive(Clone, Debug)]
pub struct BlockUpdate<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub block: LDT::Block,
}

#[derive(Clone, Debug)]
pub struct BlockStateUpdate<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub block_header: LDT::Header,
    pub state_update: GethStateUpdateVec,
}

#[derive(Clone, Debug)]
pub struct BlockLogs<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub block_header: LDT::Header,
    pub logs: Vec<LDT::Log>,
}

#[derive(Clone, Debug, Default)]
pub struct BlockHeaderEventData<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub header: LDT::Header,
    pub next_block_number: u64,
    pub next_block_timestamp: u64,
}

pub type MessageMempoolDataUpdate<LDT = KabuDataTypesEthereum> = Message<NodeMempoolDataUpdate<LDT>>;

pub type MessageBlockHeader<LDT = KabuDataTypesEthereum> = Message<BlockHeaderEventData<LDT>>;
pub type MessageBlock<LDT = KabuDataTypesEthereum> = Message<BlockUpdate<LDT>>;
pub type MessageBlockLogs<LDT = KabuDataTypesEthereum> = Message<BlockLogs<LDT>>;
pub type MessageBlockStateUpdate<LDT = KabuDataTypesEthereum> = Message<BlockStateUpdate<LDT>>;

impl<LDT: KabuDataTypes> BlockHeaderEventData<LDT> {
    pub fn new(header: LDT::Header) -> Self {
        let next_block_number = header.get_number() + 1;
        let next_block_timestamp = header.get_timestamp() + 12;
        Self { header, next_block_number, next_block_timestamp }
    }
}
