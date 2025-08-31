use crate::tx_compose::TxComposeData;
use crate::Message;
use alloy_primitives::{Bytes, TxHash, U256};
use alloy_rpc_types::TransactionRequest;
use eyre::{eyre, Result};
use kabu_types_blockchain::GethStateUpdateVec;
use kabu_types_market::PoolId;
use kabu_types_swap::Swap;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use revm::DatabaseRef;
use std::ops::Deref;

#[derive(Clone, Debug)]
pub enum TxState<N: NodePrimitives = EthPrimitives> {
    Stuffing(N::SignedTx),
    SignatureRequired(Box<TransactionRequest>),
    ReadyForBroadcast(Bytes),
    ReadyForBroadcastStuffing(Bytes),
}

impl<N: NodePrimitives> TxState<N> {
    pub fn rlp(&self) -> Result<Bytes> {
        match self {
            TxState::Stuffing(_t) => {
                // TODO: Need to implement encoding for NodePrimitives::SignedTx
                Err(eyre!("Encoding not yet implemented for NodePrimitives"))
            }
            TxState::ReadyForBroadcast(t) | TxState::ReadyForBroadcastStuffing(t) => Ok(t.clone()),
            _ => Err(eyre!("NOT_READY_FOR_BROADCAST")),
        }
    }
}

#[derive(Clone, Debug)]
pub enum SwapComposeMessage<DB, N: NodePrimitives = EthPrimitives> {
    Prepare(SwapComposeData<DB, N>),
    Estimate(SwapComposeData<DB, N>),
    Ready(SwapComposeData<DB, N>),
}

impl<DB, N: NodePrimitives> Deref for SwapComposeMessage<DB, N> {
    type Target = SwapComposeData<DB, N>;

    fn deref(&self) -> &Self::Target {
        self.data()
    }
}

impl<DB, N: NodePrimitives> SwapComposeMessage<DB, N> {
    pub fn data(&self) -> &SwapComposeData<DB, N> {
        match self {
            SwapComposeMessage::Prepare(x) | SwapComposeMessage::Estimate(x) | SwapComposeMessage::Ready(x) => x,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SwapComposeData<DB, N: NodePrimitives = EthPrimitives> {
    pub tx_compose: TxComposeData<N>,
    pub swap: Swap,
    pub prestate: Option<DB>,
    pub poststate: Option<DB>,
    pub poststate_update: Option<GethStateUpdateVec>,
    pub origin: Option<String>,
    pub tips_pct: Option<u32>,
    pub tips: Option<U256>,
}

impl<DB: Clone + 'static, N: NodePrimitives> SwapComposeData<DB, N> {
    pub fn same_stuffing(&self, others_stuffing_txs_hashes: &[TxHash]) -> bool {
        let tx_len = self.tx_compose.stuffing_txs_hashes.len();

        if tx_len != others_stuffing_txs_hashes.len() {
            false
        } else if tx_len == 0 {
            true
        } else {
            others_stuffing_txs_hashes.iter().all(|x| self.tx_compose.stuffing_txs_hashes.contains(x))
        }
    }

    pub fn cross_pools(&self, others_pools: &[PoolId]) -> bool {
        self.swap.get_pool_id_vec().iter().any(|x| others_pools.contains(x))
    }

    pub fn first_stuffing_hash(&self) -> TxHash {
        self.tx_compose.stuffing_txs_hashes.first().map_or(TxHash::default(), |x| *x)
    }

    pub fn tips_gas_ratio(&self) -> U256 {
        if self.tx_compose.gas == 0 {
            U256::ZERO
        } else {
            self.tips.unwrap_or_default() / U256::from(self.tx_compose.gas)
        }
    }

    pub fn profit_eth_gas_ratio(&self) -> U256 {
        if self.tx_compose.gas == 0 {
            U256::ZERO
        } else {
            self.swap.arb_profit_eth() / U256::from(self.tx_compose.gas)
        }
    }

    pub fn gas_price(&self) -> u128 {
        self.tx_compose.next_block_base_fee as u128 + self.tx_compose.priority_gas_fee as u128
    }

    pub fn gas_cost(&self) -> u128 {
        self.tx_compose.gas as u128 * (self.tx_compose.next_block_base_fee as u128 + self.tx_compose.priority_gas_fee as u128)
    }
}

impl<DB: DatabaseRef + Send + Sync + Clone + 'static, N: NodePrimitives> Default for SwapComposeData<DB, N> {
    fn default() -> Self {
        Self {
            tx_compose: Default::default(),
            swap: Swap::None,
            prestate: None,
            poststate: None,
            poststate_update: None,
            origin: None,
            tips_pct: None,
            tips: None,
        }
    }
}

pub type MessageSwapCompose<DB, N = EthPrimitives> = Message<SwapComposeMessage<DB, N>>;

impl<DB, N: NodePrimitives> MessageSwapCompose<DB, N> {
    pub fn prepare(data: SwapComposeData<DB, N>) -> Self {
        Message::new(SwapComposeMessage::Prepare(data))
    }

    pub fn estimate(data: SwapComposeData<DB, N>) -> Self {
        Message::new(SwapComposeMessage::Estimate(data))
    }

    pub fn ready(data: SwapComposeData<DB, N>) -> Self {
        Message::new(SwapComposeMessage::Ready(data))
    }
}
