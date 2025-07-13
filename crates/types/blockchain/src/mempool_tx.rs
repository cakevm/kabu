use alloy_primitives::{BlockNumber, TxHash};
use chrono::{DateTime, Utc};

use crate::FetchState;
use crate::{KabuDataTypes, KabuDataTypesEthereum};

#[derive(Clone, Debug)]
pub struct MempoolTx<D: KabuDataTypes> {
    pub source: String,
    pub tx_hash: D::TxHash,
    pub time: DateTime<Utc>,
    pub tx: Option<D::Transaction>,
    pub logs: Option<Vec<D::Log>>,
    pub mined: Option<BlockNumber>,
    pub failed: Option<bool>,
    pub state_update: Option<D::StateUpdate>,
    pub pre_state: Option<FetchState<D::StateUpdate>>,
}

impl MempoolTx<KabuDataTypesEthereum> {
    pub fn new() -> MempoolTx<KabuDataTypesEthereum> {
        MempoolTx { ..MempoolTx::default() }
    }
    pub fn new_with_hash(tx_hash: TxHash) -> MempoolTx<KabuDataTypesEthereum> {
        MempoolTx { tx_hash, ..MempoolTx::default() }
    }
}

impl<LDT: KabuDataTypes> Default for MempoolTx<LDT> {
    fn default() -> Self {
        MempoolTx {
            source: "unknown".to_string(),
            tx_hash: LDT::TxHash::default(),
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
