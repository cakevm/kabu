use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_storage_db::DbPool;
use kabu_types_blockchain::KabuDataTypesEthereum;
use revm::{DatabaseCommit, DatabaseRef};

#[derive(Clone)]
pub struct AppState<DB: DatabaseRef + DatabaseCommit + Clone + Send + Sync + 'static> {
    pub db: DbPool,
    pub bc: Blockchain,
    pub state: BlockchainState<DB, KabuDataTypesEthereum>,
}
