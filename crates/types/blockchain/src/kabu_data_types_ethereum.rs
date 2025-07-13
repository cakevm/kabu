use crate::kabu_data_types::KabuTransactionRequest;
use crate::{GethStateUpdate, KabuBlock, KabuDataTypes, KabuDataTypesEVM, KabuTx};
use alloy_consensus::Transaction as TransactionTrait;
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{Address, BlockHash, Bytes, TxHash, TxKind};
use alloy_provider::network::TransactionBuilder;
use alloy_provider::network::TransactionResponse;
use alloy_rpc_types_eth::{Block as EthBlock, Header, Log, Transaction, TransactionReceipt, TransactionRequest};
#[derive(Clone, Debug, Default)]
pub struct KabuDataTypesEthereum {
    _private: (),
}

impl KabuDataTypes for KabuDataTypesEthereum {
    type Transaction = Transaction;
    type TransactionRequest = TransactionRequest;
    type TransactionReceipt = TransactionReceipt;
    type Block = EthBlock;
    type Header = Header;
    type Log = Log;
    type StateUpdate = GethStateUpdate;
    type BlockHash = BlockHash;
    type TxHash = TxHash;
    type Address = Address;
}

impl KabuDataTypesEVM for KabuDataTypesEthereum {}

impl KabuTx<KabuDataTypesEthereum> for Transaction {
    fn get_gas_price(&self) -> u128 {
        TransactionTrait::max_fee_per_gas(self)
    }

    fn get_gas_limit(&self) -> u64 {
        TransactionTrait::gas_limit(self)
    }

    fn get_tx_hash(&self) -> <KabuDataTypesEthereum as KabuDataTypes>::TxHash {
        TransactionResponse::tx_hash(self)
    }

    fn get_nonce(&self) -> u64 {
        TransactionTrait::nonce(self)
    }

    fn get_from(&self) -> Address {
        TransactionResponse::from(self)
    }

    fn encode(&self) -> Vec<u8> {
        self.inner.encoded_2718()
    }

    fn to_transaction_request(&self) -> <KabuDataTypesEthereum as KabuDataTypes>::TransactionRequest {
        self.clone().into_request()
    }
}

impl KabuBlock<KabuDataTypesEthereum> for EthBlock {
    fn get_transactions(&self) -> Vec<<KabuDataTypesEthereum as KabuDataTypes>::Transaction> {
        self.transactions.as_transactions().unwrap_or_default().to_vec()
    }

    fn get_header(&self) -> <KabuDataTypesEthereum as KabuDataTypes>::Header {
        self.header.clone()
    }
}

impl KabuTransactionRequest<KabuDataTypesEthereum> for TransactionRequest {
    fn get_to(&self) -> Option<<KabuDataTypesEthereum as KabuDataTypes>::Address> {
        match &self.to {
            None => None,
            Some(tx_kind) => match tx_kind {
                TxKind::Create => None,
                TxKind::Call(to) => Some(*to),
            },
        }
    }

    fn build_call(to: <KabuDataTypesEthereum as KabuDataTypes>::Address, data: Bytes) -> TransactionRequest {
        TransactionRequest::default().with_kind(TxKind::Call(to)).with_input(data).with_gas_limit(1_000_000)
    }
}
