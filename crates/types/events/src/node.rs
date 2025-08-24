use crate::Message;
use alloy_primitives::TxHash;
use alloy_rpc_types::Header;
use kabu_types_blockchain::{GethStateUpdateVec, MempoolTx};
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use std::marker::PhantomData;

#[derive(Clone, Debug)]
pub struct NodeMempoolDataUpdate<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub tx_hash: TxHash,
    pub mempool_tx: MempoolTx<LDT>,
}

#[derive(Clone, Debug)]
pub struct BlockUpdate<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub block: LDT::Block,
}

#[derive(Clone, Debug)]
pub struct BlockStateUpdate<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub block_header: Header,
    pub state_update: GethStateUpdateVec,
    #[doc(hidden)]
    pub _phantom: PhantomData<LDT>,
}

#[derive(Clone, Debug, Default)]
pub struct BlockHeaderEventData<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    pub header: Header,
    pub next_block_number: u64,
    pub next_block_timestamp: u64,
    #[doc(hidden)]
    pub _phantom: PhantomData<LDT>,
}

pub type MessageMempoolDataUpdate<LDT = KabuDataTypesEthereum> = Message<NodeMempoolDataUpdate<LDT>>;

pub type MessageBlockHeader<LDT = KabuDataTypesEthereum> = Message<BlockHeaderEventData<LDT>>;
pub type MessageBlock<LDT = KabuDataTypesEthereum> = Message<BlockUpdate<LDT>>;
pub type MessageBlockStateUpdate<LDT = KabuDataTypesEthereum> = Message<BlockStateUpdate<LDT>>;

impl<LDT: KabuDataTypes> BlockHeaderEventData<LDT> {
    pub fn new(header: Header) -> Self {
        let next_block_number = header.number + 1;
        let next_block_timestamp = header.timestamp + 12;
        Self { header, next_block_number, next_block_timestamp, _phantom: PhantomData }
    }
}
