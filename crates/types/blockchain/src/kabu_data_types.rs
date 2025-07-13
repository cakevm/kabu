use crate::{ChainParameters, GethStateUpdate};
use alloy_consensus::BlockHeader;
use alloy_primitives::{Address, BlockHash, Bytes, TxHash};
use alloy_rpc_types::TransactionTrait;
use alloy_rpc_types_eth::{Header, Log};
use std::fmt::{Debug, Display};
use std::hash::Hash;

pub trait KabuTx<LDT: KabuDataTypes> {
    fn get_gas_price(&self) -> u128;
    fn get_gas_limit(&self) -> u64;

    fn get_tx_hash(&self) -> LDT::TxHash;

    fn get_nonce(&self) -> u64;
    fn get_from(&self) -> LDT::Address;

    fn encode(&self) -> Vec<u8>;

    fn to_transaction_request(&self) -> LDT::TransactionRequest;
}

pub trait KabuHeader<LDT: KabuDataTypes> {
    fn get_timestamp(&self) -> u64;
    fn get_number(&self) -> u64;

    fn get_hash(&self) -> LDT::BlockHash;
    fn get_parent_hash(&self) -> LDT::BlockHash;

    fn get_base_fee(&self) -> Option<u128>;

    fn get_next_base_fee(&self, params: &ChainParameters) -> u128;

    fn get_beneficiary(&self) -> LDT::Address;
}

pub trait KabuBlock<LDT: KabuDataTypes> {
    fn get_transactions(&self) -> Vec<LDT::Transaction>;

    fn get_header(&self) -> LDT::Header;
}

pub trait KabuTransactionRequest<LDT: KabuDataTypes> {
    fn get_to(&self) -> Option<LDT::Address>;
    fn build_call(to: LDT::Address, data: Bytes) -> LDT::TransactionRequest;
}

pub trait KabuDataTypes: Debug + Clone + Send + Sync {
    type Transaction: Debug + Clone + Send + Sync + KabuTx<Self> + TransactionTrait;
    type TransactionRequest: Debug + Clone + Send + Sync + KabuTransactionRequest<Self>;
    type TransactionReceipt: Debug + Clone + Send + Sync;
    type Block: Default + Debug + Clone + Send + Sync + KabuBlock<Self>;
    type Header: Default + Debug + Clone + Send + Sync + KabuHeader<Self>;
    type Log: Default + Debug + Clone + Send + Sync;
    type StateUpdate: Default + Debug + Clone + Send + Sync;
    type BlockHash: Eq + Copy + Hash + Default + Display + Debug + Clone + Send + Sync;
    type TxHash: Eq + Copy + Hash + Default + Display + Debug + Clone + Send + Sync;
    type Address: Eq + Copy + Hash + Ord + Default + Display + Debug + Clone + Send + Sync;
}

pub trait KabuDataTypesEVM:
    KabuDataTypes<Header = Header, TxHash = TxHash, BlockHash = BlockHash, Log = Log, StateUpdate = GethStateUpdate, Address = Address>
{
}

impl<LDT> KabuHeader<LDT> for Header
where
    LDT: KabuDataTypes<Header = Header, BlockHash = BlockHash, Address = Address>,
{
    fn get_timestamp(&self) -> u64 {
        self.timestamp
    }

    fn get_number(&self) -> u64 {
        self.number
    }

    fn get_hash(&self) -> LDT::BlockHash {
        self.hash
    }

    fn get_parent_hash(&self) -> LDT::BlockHash {
        self.parent_hash
    }

    fn get_base_fee(&self) -> Option<u128> {
        self.base_fee_per_gas().map(|s| s as u128)
    }

    fn get_next_base_fee(&self, params: &ChainParameters) -> u128 {
        params.calc_next_block_base_fee_from_header(self) as u128
    }

    fn get_beneficiary(&self) -> LDT::Address {
        self.beneficiary
    }
}
