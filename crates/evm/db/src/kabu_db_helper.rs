use crate::error::KabuDBError;
use alloy::primitives::{Address, BlockNumber, B256, U256};
use eyre::eyre;
use revm::state::{AccountInfo, Bytecode};
use revm::DatabaseRef;
use tracing::trace;

pub struct KabuDBHelper {}

impl KabuDBHelper {
    #[inline]
    pub fn get_code_by_hash<DB: DatabaseRef<Error = KabuDBError>>(
        read_only_db: &Option<DB>,
        code_hash: B256,
    ) -> eyre::Result<Bytecode, KabuDBError> {
        match read_only_db {
            Some(read_only_db) => read_only_db.code_by_hash_ref(code_hash),
            None => Err(KabuDBError::NoDB),
        }
    }

    #[inline]
    fn fetch_storage<ExtDB: DatabaseRef<Error = KabuDBError>>(
        ext_db: &Option<ExtDB>,
        address: Address,
        slot: U256,
    ) -> Result<U256, KabuDBError> {
        trace!(%address, %slot, "fetch_storage");

        if let Some(ext_db) = ext_db {
            let value = ext_db.storage_ref(address, slot).map_err(|_| eyre!("ERROR_READING_ALLOY_DB"));
            trace!(%address, %slot, ?value , "fetch_storage returned");
            Ok(value.unwrap_or_default())
        } else {
            trace!("fetch_storage returned NO_EXT_DB");
            Err(KabuDBError::NoDB)
        }
    }

    #[inline]
    pub fn get_or_fetch_storage<DB: DatabaseRef<Error = KabuDBError>, ExtDB: DatabaseRef<Error = KabuDBError>>(
        read_only_db: &Option<DB>,
        ext_db: &Option<ExtDB>,
        address: Address,
        slot: U256,
    ) -> Result<U256, KabuDBError> {
        trace!(%address, %slot, "get_or_fetch_storage");

        match read_only_db {
            Some(read_only_db) => {
                let value = read_only_db.storage_ref(address, slot).or_else(|_| Self::fetch_storage(ext_db, address, slot));
                trace!(%address, %slot, ?value , "get_or_fetch_storage with RO");
                value
            }
            None => {
                let value = Self::fetch_storage(ext_db, address, slot);
                trace!(%address, %slot, ?value , "get_or_fetch_storage without RO");
                value
            }
        }
    }

    #[inline]
    fn fetch_basic<ExtDB: DatabaseRef<Error = KabuDBError>>(
        ext_db: &Option<ExtDB>,
        address: Address,
    ) -> Result<Option<AccountInfo>, KabuDBError> {
        trace!(%address, "fetch_basic");
        if let Some(ext_db) = ext_db {
            ext_db.basic_ref(address)
        } else {
            Err(KabuDBError::NoDB)
        }
    }

    #[inline]
    pub fn get_basic<DB: DatabaseRef<Error = KabuDBError>>(
        read_only_db: &Option<DB>,
        address: Address,
    ) -> eyre::Result<Option<AccountInfo>> {
        read_only_db
            .as_ref()
            .and_then(|db| db.basic_ref(address).ok().flatten())
            .map_or_else(|| Err(eyre!("NO_ACCOUNT")), |info| Ok(Some(info)))
    }

    #[inline]
    pub fn get_or_fetch_basic<DB: DatabaseRef<Error = KabuDBError>, ExtDB: DatabaseRef<Error = KabuDBError>>(
        read_only_db: &Option<DB>,
        ext_db: &Option<ExtDB>,
        address: Address,
    ) -> Result<Option<AccountInfo>, KabuDBError> {
        trace!(%address, "get_or_fetch_basic");
        match &read_only_db {
            Some(read_only_db) => match read_only_db.basic_ref(address) {
                Ok(Some(info)) => Ok(Some(info)),
                _ => Self::fetch_basic(ext_db, address),
            },
            None => Self::fetch_basic(ext_db, address),
        }
    }

    #[inline]
    fn fetch_block_hash_ref<ExtDB: DatabaseRef<Error = KabuDBError>>(
        ext_db: &Option<ExtDB>,
        number: BlockNumber,
    ) -> Result<B256, KabuDBError> {
        if let Some(ext_db) = ext_db {
            ext_db.block_hash_ref(number)
        } else {
            Err(KabuDBError::NoDB)
        }
    }

    #[inline]
    pub fn get_or_fetch_block_hash<DB: DatabaseRef<Error = KabuDBError>, ExtDB: DatabaseRef<Error = KabuDBError>>(
        read_only_db: &Option<DB>,
        ext_db: &Option<ExtDB>,
        number: BlockNumber,
    ) -> Result<B256, KabuDBError> {
        match read_only_db {
            Some(read_only_db) => read_only_db.block_hash_ref(number).or_else(|_| KabuDBHelper::fetch_block_hash_ref(ext_db, number)),
            None => Self::fetch_block_hash_ref(ext_db, number),
        }
    }
}
