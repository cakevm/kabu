use crate::{Message, TxState};
use alloy_primitives::{Address, BlockNumber, Bytes, TxHash, U256};
use kabu_types_entities::LoomTxSigner;
use kabu_types_swap::Swap;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum RlpState {
    Stuffing(Bytes),
    Backrun(Bytes),
    None,
}

impl RlpState {
    pub fn is_none(&self) -> bool {
        matches!(self, RlpState::None)
    }

    pub fn unwrap(&self) -> Bytes {
        match self.clone() {
            RlpState::Backrun(val) | RlpState::Stuffing(val) => val,
            RlpState::None => Bytes::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum TxComposeMessageType<N: NodePrimitives = EthPrimitives> {
    Sign(TxComposeData<N>),
    Broadcast(TxComposeData<N>),
}

#[derive(Clone, Debug)]
pub struct TxComposeData<N: NodePrimitives = EthPrimitives> {
    /// The EOA address that will be used to sign the transaction.
    /// If this is None, the transaction will be signed by a random signer.
    pub eoa: Option<Address>,
    pub signer: Option<Arc<dyn LoomTxSigner<N>>>,
    pub nonce: u64,
    pub eth_balance: U256,
    pub value: U256,
    pub gas: u64,
    pub priority_gas_fee: u64,
    pub stuffing_txs_hashes: Vec<TxHash>,
    pub stuffing_txs: Vec<N::SignedTx>,
    pub next_block_number: BlockNumber,
    pub next_block_timestamp: u64,
    pub next_block_base_fee: u64,
    pub tx_bundle: Option<Vec<TxState<N>>>,
    pub rlp_bundle: Option<Vec<RlpState>>,
    pub origin: Option<String>,
    pub swap: Option<Swap>,
    pub tips: Option<U256>,
}

impl<N: NodePrimitives> Default for TxComposeData<N> {
    fn default() -> Self {
        Self {
            eoa: None,
            signer: None,
            nonce: Default::default(),
            eth_balance: Default::default(),
            next_block_base_fee: Default::default(),
            value: Default::default(),
            gas: Default::default(),
            priority_gas_fee: Default::default(),
            stuffing_txs_hashes: Vec::new(),
            stuffing_txs: Vec::new(),
            next_block_number: Default::default(),
            next_block_timestamp: Default::default(),
            tx_bundle: None,
            rlp_bundle: None,
            origin: None,
            swap: None,
            tips: None,
        }
    }
}

pub type MessageTxCompose<N = EthPrimitives> = Message<TxComposeMessageType<N>>;

impl<N: NodePrimitives> MessageTxCompose<N> {
    pub fn sign(data: TxComposeData<N>) -> Self {
        Message::new(TxComposeMessageType::Sign(data))
    }

    pub fn broadcast(data: TxComposeData<N>) -> Self {
        Message::new(TxComposeMessageType::Broadcast(data))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let v = [RlpState::Stuffing(Bytes::from(vec![1])), RlpState::Backrun(Bytes::from(vec![2]))];

        let b: Vec<Bytes> = v.iter().filter(|i| matches!(i, RlpState::Backrun(_))).map(|i| i.unwrap()).collect();

        for c in b {
            println!("{c:?}");
        }
    }
}
