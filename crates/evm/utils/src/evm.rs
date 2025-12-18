use crate::evm_env::{block_env_from_block_header, tx_evm_env_from_tx};
use alloy_eips::BlockNumHash;
use alloy_evm::EvmEnv;
use alloy_primitives::{Address, B256, Bytes};
use alloy_primitives::{TxHash, TxKind, address};
use alloy_rpc_types::Log;
use alloy_rpc_types::{AccessList, AccessListItem, Header, Transaction};
use alloy_rpc_types_trace::geth::AccountState;
use eyre::eyre;
use kabu_types_blockchain::GethStateUpdate;
use revm::ExecuteEvm;
use revm::context::result::{EVMError, ExecutionResult, HaltReason, Output, ResultAndState};
use revm::context::{ContextTr, TxEnv};
use revm::database::CacheDB;
use revm::handler::EvmTr;
use revm::state::{Account, EvmState};
use revm::{Context, Database, DatabaseCommit, DatabaseRef, MainBuilder, MainContext};
use std::collections::BTreeMap;
use thiserror::Error;
use tracing::debug;

pub static COINBASE: Address = address!("1f9090aaE28b8a3dCeaDf281B0F12828e676c326");

#[derive(Debug, Error)]
pub enum EvmError {
    #[error("Evm transact error with err={0}")]
    TransactError(String),
    #[error("Evm transact commit error with err={0}")]
    TransactCommitError(String),
    #[error("Reverted with reason={0}, gas_used={1}")]
    Reverted(String, u64),
    #[error("Halted with halt_reason={0:?}, gas_used={1}")]
    Halted(HaltReason, u64),
}

fn parse_execution_result(execution_result: ResultAndState) -> eyre::Result<(Vec<u8>, u64, EvmState)> {
    let ResultAndState { result, state } = execution_result;
    let gas_used = result.gas_used();

    match result {
        ExecutionResult::Success { output: Output::Call(value), .. } => Ok((value.to_vec(), gas_used, state)),
        ExecutionResult::Success { output: Output::Create(_bytes, _address), .. } => Ok((vec![], gas_used, state)),
        ExecutionResult::Revert { output, gas_used } => Err(eyre!(EvmError::Reverted(revert_bytes_to_string(&output), gas_used))),
        ExecutionResult::Halt { reason, gas_used } => Err(eyre!(EvmError::Halted(reason, gas_used))),
    }
}

// Execute a call without any block validations
pub fn evm_call<DB>(state_db: DB, env: EvmEnv, transact_to: Address, call_data_vec: Vec<u8>) -> eyre::Result<(Vec<u8>, u64, EvmState)>
where
    DB: DatabaseRef,
    <DB as DatabaseRef>::Error: std::fmt::Debug,
{
    let mut env = env;
    env.cfg_env.disable_base_fee = true;
    env.cfg_env.disable_balance_check = true;
    env.cfg_env.disable_block_gas_limit = true;
    env.cfg_env.disable_nonce_check = true;

    let mut evm = Context::mainnet().with_db(CacheDB::new(state_db)).with_block(env.block_env).with_cfg(env.cfg_env).build_mainnet();

    let tx_env = TxEnv { kind: TxKind::Call(transact_to), data: Bytes::from(call_data_vec), ..TxEnv::default() };

    let result_and_state = evm.transact(tx_env).map_err(|e| EvmError::TransactError(format!("{e:?}")))?;

    parse_execution_result(result_and_state)
}

pub fn evm_transact<DB, EVM>(evm: &mut EVM, tx_env: TxEnv) -> eyre::Result<(Vec<u8>, u64, EvmState)>
where
    DB: Database + DatabaseCommit,
    <DB as Database>::Error: std::fmt::Debug,
    EVM: EvmTr<Context: ContextTr<Db = DB>>
        + ExecuteEvm<Tx = TxEnv, ExecutionResult = ExecutionResult, State = EvmState, Error = EVMError<<DB as Database>::Error>>,
    <EVM as ExecuteEvm>::Error: std::fmt::Debug,
    <EVM as ExecuteEvm>::State: Clone,
{
    let result = evm.transact(tx_env).map_err(|e| EvmError::TransactError(format!("{e:?}")))?;

    evm.ctx_mut().db_mut().commit(result.state.clone());
    parse_execution_result(ResultAndState { result: result.result, state: result.state })
}

// Simulate a full tx with all validations
pub fn evm_call_tx<DB>(state_db: DB, env: &EvmEnv, tx_env: TxEnv) -> eyre::Result<(Vec<u8>, u64, EvmState)>
where
    DB: DatabaseRef,
    <DB as DatabaseRef>::Error: std::fmt::Debug,
{
    let mut evm = Context::mainnet().with_db(CacheDB::new(state_db)).with_block(env.block_env.clone()).build_mainnet();

    let result_and_state = evm.transact(tx_env).map_err(|e| EvmError::TransactError(format!("{e:?}")))?;

    parse_execution_result(result_and_state)
}

pub fn evm_access_list<DB>(state_db: DB, env: &EvmEnv, tx_env: TxEnv) -> eyre::Result<(u64, AccessList)>
where
    DB: DatabaseRef,
    <DB as DatabaseRef>::Error: std::fmt::Debug,
{
    let mut env = env.clone();
    env.block_env.beneficiary = COINBASE;

    let mut evm = Context::mainnet().with_db(CacheDB::new(state_db)).with_block(env.block_env).with_cfg(env.cfg_env).build_mainnet();

    let ref_tx = evm.transact(tx_env).map_err(|e| EvmError::TransactError(format!("{e:?}")))?;
    let execution_result = ref_tx.result;
    match execution_result {
        ExecutionResult::Success { output, gas_used, reason, .. } => {
            debug!(gas_used, ?reason, ?output, "AccessList");
            let mut acl = AccessList::default();

            for (addr, acc) in ref_tx.state {
                let storage_keys: Vec<B256> = acc.storage.keys().map(|x| (*x).into()).collect();
                acl.0.push(AccessListItem { address: addr, storage_keys });
            }

            Ok((gas_used, acl))
        }
        ExecutionResult::Revert { output, gas_used } => Err(eyre!(EvmError::Reverted(revert_bytes_to_string(&output), gas_used))),
        ExecutionResult::Halt { reason, gas_used } => Err(eyre!(EvmError::Halted(reason, gas_used))),
    }
}

pub fn evm_call_tx_in_block<DB, T: Into<Transaction>>(tx: T, state_db: DB, header: &Header) -> eyre::Result<ResultAndState>
where
    DB: DatabaseRef,
{
    let mut evm = Context::mainnet().with_db(CacheDB::new(state_db)).with_block(block_env_from_block_header(header)).build_mainnet();

    let tx_env = tx_evm_env_from_tx(tx);
    evm.transact(tx_env).map_err(|_| eyre!("TRANSACT_ERROR"))
}

pub fn convert_evm_result_to_rpc(
    result: ResultAndState,
    tx_hash: TxHash,
    block_num_hash: BlockNumHash,
    block_timestamp: u64,
) -> eyre::Result<(Vec<Log>, GethStateUpdate)> {
    let logs = match result.result {
        ExecutionResult::Success { logs, .. } => logs
            .into_iter()
            .enumerate()
            .map(|(log_index, l)| Log {
                inner: l.clone(),
                block_hash: Some(block_num_hash.hash),
                block_number: Some(block_num_hash.number),
                transaction_hash: Some(tx_hash),
                transaction_index: Some(0u64),
                log_index: Some(log_index as u64),
                removed: false,
                block_timestamp: Some(block_timestamp),
            })
            .collect(),
        _ => return Err(eyre!("EXECUTION_REVERTED")),
    };

    let mut state_update: GethStateUpdate = GethStateUpdate::default();

    for (address, account) in result.state.into_iter() {
        let (address, account): (Address, Account) = (address, account);
        let storage: BTreeMap<B256, B256> = account.storage.into_iter().map(|(k, v)| (k.into(), v.present_value.into())).collect();

        let account_state = AccountState {
            balance: Some(account.info.balance),
            code: account.info.code.map(|x| x.bytes()),
            nonce: Some(account.info.nonce),
            storage,
        };
        state_update.insert(address, account_state);
    }

    Ok((logs, state_update))
}

pub fn revert_bytes_to_string(bytes: &Bytes) -> String {
    if bytes.len() < 4 {
        return format!("{bytes:?}");
    }
    let error_data = &bytes[4..];

    match String::from_utf8(error_data.to_vec()) {
        Ok(s) => s.replace(char::from(0), "").trim().to_string(),
        Err(_) => format!("{bytes:?}"),
    }
}
