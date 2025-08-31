#![allow(clippy::type_complexity)]

use alloy_consensus::Header;
use alloy_primitives::TxHash;
use kabu_types_blockchain::GethStateUpdateVec;
use kabu_types_market::PoolWrapper;
use kabu_types_market::SwapDirection;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use revm::DatabaseRef;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct StateUpdateEvent<DB, N: NodePrimitives = EthPrimitives> {
    pub next_block_number: u64,
    pub next_block_timestamp: u64,
    pub next_base_fee: u64,
    market_state: DB,
    state_update: GethStateUpdateVec,
    state_required: Option<GethStateUpdateVec>,
    directions: BTreeMap<PoolWrapper, Vec<SwapDirection>>,
    pub stuffing_txs_hashes: Vec<TxHash>,
    pub stuffing_txs: Vec<N::SignedTx>,
    pub origin: String,
    pub tips_pct: u32,
}

#[allow(clippy::too_many_arguments)]
impl<DB: DatabaseRef, N: NodePrimitives> StateUpdateEvent<DB, N> {
    pub fn new(
        next_block: u64,
        next_block_timestamp: u64,
        next_base_fee: u64,
        market_state: DB,
        state_update: GethStateUpdateVec,
        state_required: Option<GethStateUpdateVec>,
        directions: BTreeMap<PoolWrapper, Vec<SwapDirection>>,
        stuffing_txs_hashes: Vec<TxHash>,
        stuffing_txs: Vec<N::SignedTx>,
        origin: String,
        tips_pct: u32,
    ) -> StateUpdateEvent<DB, N> {
        StateUpdateEvent {
            next_block_number: next_block,
            next_block_timestamp,
            next_base_fee,
            state_update,
            state_required,
            market_state,
            directions,
            stuffing_txs_hashes,
            stuffing_txs,
            origin,
            tips_pct,
        }
    }
    pub fn directions(&self) -> &BTreeMap<PoolWrapper, Vec<SwapDirection>> {
        &self.directions
    }

    pub fn market_state(&self) -> &DB {
        &self.market_state
    }

    pub fn state_update(&self) -> &GethStateUpdateVec {
        &self.state_update
    }

    pub fn state_required(&self) -> &Option<GethStateUpdateVec> {
        &self.state_required
    }

    pub fn stuffing_len(&self) -> usize {
        self.stuffing_txs_hashes.len()
    }

    pub fn stuffing_txs_hashes(&self) -> &Vec<TxHash> {
        &self.stuffing_txs_hashes
    }
    pub fn stuffing_txs(&self) -> &Vec<N::SignedTx> {
        &self.stuffing_txs
    }

    pub fn stuffing_tx_hash(&self) -> TxHash {
        self.stuffing_txs_hashes.first().cloned().unwrap_or_default()
    }
}

impl<DB: DatabaseRef, N: NodePrimitives> StateUpdateEvent<DB, N> {
    pub fn next_header(&self) -> Header {
        Header { number: self.next_block_number, timestamp: self.next_block_timestamp, ..Default::default() }
    }
}
