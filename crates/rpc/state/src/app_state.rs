use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_storage_db::DbPool;
use reth_ethereum_primitives::EthPrimitives;
use revm::{DatabaseCommit, DatabaseRef};

#[derive(Clone)]
pub struct AppState<DB: DatabaseRef + DatabaseCommit + Clone + Send + Sync + 'static> {
    pub db: DbPool,
    pub bc: Blockchain,
    pub state: BlockchainState<DB, EthPrimitives>,
}
