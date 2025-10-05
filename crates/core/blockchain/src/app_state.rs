use alloy::primitives::ChainId;
use kabu_types_blockchain::ChainParameters;
use kabu_types_blockchain::Mempool;
use kabu_types_entities::AccountNonceAndBalanceState;
use kabu_types_market::Market;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState<N: NodePrimitives = EthPrimitives> {
    pub chain_id: ChainId,
    pub chain_parameters: ChainParameters,
    pub market: Arc<RwLock<Market>>,
    pub mempool: Arc<RwLock<Mempool<N>>>,
    pub account_nonce_and_balance: Arc<RwLock<AccountNonceAndBalanceState>>,
}

impl<N: NodePrimitives> AppState<N> {
    pub fn new(chain_id: ChainId) -> Self {
        let chain_parameters = ChainParameters::ethereum();

        Self {
            chain_id,
            chain_parameters,
            market: Arc::new(RwLock::new(Market::default())),
            mempool: Arc::new(RwLock::new(Default::default())),
            account_nonce_and_balance: Arc::new(RwLock::new(AccountNonceAndBalanceState::new())),
        }
    }

    pub fn with_chain_parameters(mut self, chain_parameters: ChainParameters) -> Self {
        self.chain_parameters = chain_parameters;
        self
    }
}
