use crate::Message;
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_entities::{EstimationError, SwapError};

#[derive(Clone, Debug)]
pub enum HealthEvent<LDT: KabuDataTypes = KabuDataTypesEthereum> {
    PoolSwapError(SwapError),
    SwapLineEstimationError(EstimationError),
    MonitorTx(LDT::TxHash),
}

pub type MessageHealthEvent<LDT = KabuDataTypesEthereum> = Message<HealthEvent<LDT>>;
