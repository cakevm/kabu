use kabu_evm_db::DatabaseKabuExt;
use kabu_types_blockchain::KabuDataTypes;
use kabu_types_entities::{BlockHistory, BlockHistoryState};
use kabu_types_market::MarketState;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct BlockchainState<DB: Clone + Send + Sync + 'static, LDT: KabuDataTypes> {
    market_state: Arc<RwLock<MarketState<DB>>>,
    block_history_state: Arc<RwLock<BlockHistory<DB, LDT>>>,
}

impl<
        DB: DatabaseRef + Database + DatabaseCommit + BlockHistoryState<LDT> + DatabaseKabuExt + Send + Sync + Clone + Default + 'static,
        LDT: KabuDataTypes,
    > Default for BlockchainState<DB, LDT>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
        DB: DatabaseRef + Database + DatabaseCommit + BlockHistoryState<LDT> + DatabaseKabuExt + Send + Sync + Clone + Default + 'static,
        LDT: KabuDataTypes,
    > BlockchainState<DB, LDT>
{
    pub fn new() -> Self {
        BlockchainState {
            market_state: Arc::new(RwLock::new(MarketState::new(DB::default()))),
            block_history_state: Arc::new(RwLock::new(BlockHistory::<DB, LDT>::new(10))),
        }
    }

    pub fn new_with_market_state(market_state: MarketState<DB>) -> Self {
        Self { market_state: Arc::new(RwLock::new(market_state)), block_history_state: Arc::new(RwLock::new(BlockHistory::new(10))) }
    }

    pub fn with_market_state(self, market_state: MarketState<DB>) -> BlockchainState<DB, LDT> {
        BlockchainState { market_state: Arc::new(RwLock::new(market_state)), ..self.clone() }
    }
}

impl<DB: Clone + Send + Sync, LDT: KabuDataTypes> BlockchainState<DB, LDT> {
    pub fn market_state_commit(&self) -> Arc<RwLock<MarketState<DB>>> {
        self.market_state.clone()
    }

    pub fn market_state(&self) -> Arc<RwLock<MarketState<DB>>> {
        self.market_state.clone()
    }

    pub fn block_history(&self) -> Arc<RwLock<BlockHistory<DB, LDT>>> {
        self.block_history_state.clone()
    }
}
