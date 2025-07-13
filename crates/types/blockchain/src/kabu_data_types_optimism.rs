use crate::kabu_data_types::KabuTransactionRequest;
use crate::{GethStateUpdate, KabuBlock, KabuDataTypes, KabuDataTypesEVM, KabuDataTypesEthereum, KabuTx};
use alloy_consensus::Transaction as TransactionTrait;
use alloy_primitives::{Address, BlockHash, Bytes, TxHash, TxKind};
use alloy_provider::network::{TransactionBuilder, TransactionResponse};
use alloy_rpc_types_eth::{Block as EthBlock, Header, Log};
use op_alloy::rpc_types::{OpTransactionReceipt, OpTransactionRequest, Transaction as OpTransaction};

#[derive(Clone, Debug, Default)]
pub struct KabuDataTypesOptimism {
    _private: (),
}

impl KabuDataTypes for KabuDataTypesOptimism {
    type Transaction = OpTransaction;
    type TransactionRequest = OpTransactionRequest;
    type TransactionReceipt = OpTransactionReceipt;
    type Block = EthBlock<OpTransaction, Header>;
    type Header = Header;
    type Log = Log;
    type StateUpdate = GethStateUpdate;
    type BlockHash = BlockHash;
    type TxHash = TxHash;
    type Address = Address;
}

impl KabuDataTypesEVM for KabuDataTypesOptimism {}

impl KabuTx<KabuDataTypesOptimism> for OpTransaction {
    fn get_gas_price(&self) -> u128 {
        TransactionTrait::max_fee_per_gas(self)
    }

    fn get_gas_limit(&self) -> u64 {
        TransactionTrait::gas_limit(self)
    }

    fn get_tx_hash(&self) -> <KabuDataTypesOptimism as KabuDataTypes>::TxHash {
        TransactionResponse::tx_hash(self)
    }

    fn get_nonce(&self) -> u64 {
        TransactionTrait::nonce(self)
    }

    fn get_from(&self) -> Address {
        TransactionResponse::from(self)
    }

    fn encode(&self) -> Vec<u8> {
        //self.inner.
        //TODO : Fix this
        vec![]
    }

    fn to_transaction_request(&self) -> <KabuDataTypesOptimism as KabuDataTypes>::TransactionRequest {
        let r = self.inner.clone().into_inner();
        r.into()
    }
}

impl KabuBlock<KabuDataTypesOptimism> for EthBlock<OpTransaction, Header> {
    fn get_transactions(&self) -> Vec<<KabuDataTypesOptimism as KabuDataTypes>::Transaction> {
        self.transactions.clone().into_transactions_vec()
    }

    fn get_header(&self) -> <KabuDataTypesOptimism as KabuDataTypes>::Header {
        self.header.clone()
    }
}

impl KabuTransactionRequest<KabuDataTypesOptimism> for OpTransactionRequest {
    fn get_to(&self) -> Option<<KabuDataTypesOptimism as KabuDataTypes>::Address> {
        match &self.clone().build_typed_tx() {
            Ok(tx) => tx.to(),
            _ => None,
        }
    }

    fn build_call(to: <KabuDataTypesEthereum as KabuDataTypes>::Address, data: Bytes) -> OpTransactionRequest {
        OpTransactionRequest::default().with_kind(TxKind::Call(to)).with_input(data)
    }
}
