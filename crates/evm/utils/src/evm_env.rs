use alloy::consensus::BlockHeader;
use alloy::network::TransactionBuilder;
use alloy::primitives::{Address, U256};
use alloy::rpc::types::TransactionRequest;
use alloy_eips::Typed2718;
use alloy_primitives::TxKind;
use alloy_rpc_types::{AccessList, Transaction, TransactionTrait};
use alloy_rpc_types_eth::Header;
use either::Either;
use lazy_static::lazy_static;
use revm::context::{BlockEnv, TxEnv};
use revm::context_interface::block::BlobExcessGasAndPrice;

lazy_static! {
    static ref COINBASE: Address = "0x1f9090aaE28b8a3dCeaDf281B0F12828e676c326".parse().unwrap();
}
/*
pub async fn env_fetch_for_block<P: Provider<T, N>, T: Transport + Clone, N: Network>(provider: P, BlockID: BlockId) -> Result<Env> {
    let block = provider.get_block_by_number()
}

 */

pub fn tx_req_to_env<T: Into<TransactionRequest>>(tx: T) -> TxEnv {
    let tx: TransactionRequest = tx.into();
    TxEnv {
        tx_type: tx.transaction_type.unwrap_or_default(),
        caller: tx.from.unwrap_or_default(),
        kind: tx.kind().unwrap_or_default(),
        gas_limit: tx.gas.unwrap_or_default(),
        gas_price: tx.max_fee_per_gas.unwrap_or_default(),
        value: tx.value.unwrap_or_default(),
        data: tx.input.input().cloned().unwrap_or_default(),
        nonce: tx.nonce.unwrap_or_default(),
        chain_id: tx.chain_id(),
        access_list: tx.access_list.unwrap_or_default(),
        gas_priority_fee: tx.max_priority_fee_per_gas,
        blob_hashes: tx.blob_versioned_hashes.unwrap_or_default(),
        max_fee_per_blob_gas: tx.max_fee_per_blob_gas.unwrap_or_default(),
        authorization_list: tx.authorization_list.unwrap_or_default().into_iter().map(Either::Left).collect(),
    }
}

pub fn header_to_block_env<H: BlockHeader>(header: &H) -> BlockEnv {
    BlockEnv {
        number: U256::from(header.number()),
        beneficiary: header.beneficiary(),
        timestamp: U256::from(header.timestamp()),
        gas_limit: header.gas_limit(),
        basefee: header.base_fee_per_gas().unwrap_or_default(),
        difficulty: header.difficulty(),
        prevrandao: Some(header.parent_hash()),
        blob_excess_gas_and_price: Some(BlobExcessGasAndPrice::new(header.excess_blob_gas().unwrap_or_default(), 3338477u64)),
    }
}

pub fn block_env_from_block_header(block_header: &Header) -> BlockEnv {
    BlockEnv {
        number: U256::from(block_header.number),
        beneficiary: block_header.beneficiary,
        timestamp: U256::from(block_header.timestamp),
        gas_limit: block_header.gas_limit,
        basefee: block_header.base_fee_per_gas.unwrap_or_default(),
        difficulty: block_header.difficulty,
        prevrandao: Some(block_header.parent_hash),
        blob_excess_gas_and_price: None,
    }
}

pub fn tx_evm_env_from_tx<T: Into<Transaction>>(tx: T) -> TxEnv {
    let tx = tx.into();

    TxEnv {
        tx_type: tx.inner.tx_type().ty(),
        caller: tx.inner.signer(),
        gas_limit: tx.gas_limit(),
        gas_price: tx.max_fee_per_gas(),
        kind: match tx.to() {
            Some(to_address) => TxKind::Call(to_address),
            None => TxKind::Create,
        },
        value: tx.value(),
        data: tx.input().clone(),
        nonce: tx.nonce(),
        chain_id: tx.chain_id(),
        access_list: AccessList::default(),
        gas_priority_fee: tx.max_priority_fee_per_gas(),
        blob_hashes: Vec::new(),
        max_fee_per_blob_gas: 0,
        authorization_list: vec![],
    }
}
