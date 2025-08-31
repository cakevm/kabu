use crate::{FetchState, GethStateUpdate};
use alloy_primitives::{BlockNumber, TxHash};
use alloy_rpc_types_eth::Log;
use chrono::{DateTime, Utc};
use reth_node_types::NodePrimitives;

#[derive(Clone, Debug)]
pub struct MempoolTx<N: NodePrimitives> {
    pub source: String,
    pub tx_hash: TxHash,
    pub time: DateTime<Utc>,
    pub tx: Option<N::SignedTx>,
    pub logs: Option<Vec<Log>>,
    pub mined: Option<BlockNumber>,
    pub failed: Option<bool>,
    pub state_update: Option<GethStateUpdate>,
    pub pre_state: Option<FetchState<GethStateUpdate>>,
}

impl<N: NodePrimitives> MempoolTx<N> {
    pub fn new() -> MempoolTx<N> {
        MempoolTx { ..MempoolTx::default() }
    }
    pub fn new_with_hash(tx_hash: TxHash) -> MempoolTx<N> {
        MempoolTx { tx_hash, ..MempoolTx::default() }
    }
}

impl<N: NodePrimitives> Default for MempoolTx<N> {
    fn default() -> Self {
        MempoolTx {
            source: "unknown".to_string(),
            tx_hash: TxHash::default(),
            time: Utc::now(),
            tx: None,
            state_update: None,
            logs: None,
            mined: None,
            failed: None,
            pre_state: None,
        }
    }
}
