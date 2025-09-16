use alloy::primitives::ChainId;
use kabu_types_blockchain::ChainParameters;
use kabu_types_blockchain::Mempool;
use kabu_types_entities::AccountNonceAndBalanceState;
use kabu_types_market::Market;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

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

#[derive(Clone)]
pub struct EventChannels<N: NodePrimitives = EthPrimitives> {
    pub new_block_headers: broadcast::Sender<kabu_types_events::MessageBlockHeader<N>>,
    pub new_block_with_tx: broadcast::Sender<kabu_types_events::MessageBlock<N>>,
    pub new_block_state_update: broadcast::Sender<kabu_types_events::MessageBlockStateUpdate<N>>,
    pub new_mempool_tx: broadcast::Sender<kabu_types_events::MessageMempoolDataUpdate<N>>,
    pub market_events: broadcast::Sender<kabu_types_events::MarketEvents>,
    pub mempool_events: broadcast::Sender<kabu_types_events::MempoolEvents>,
    pub tx_compose: broadcast::Sender<kabu_types_events::MessageTxCompose<N>>,
    pub pool_health_monitor: broadcast::Sender<kabu_types_events::MessageHealthEvent>,
    pub influxdb_write: Option<broadcast::Sender<influxdb::WriteQuery>>,
    pub tasks: broadcast::Sender<kabu_types_events::LoomTask>,
}

impl<N: NodePrimitives> Default for EventChannels<N> {
    fn default() -> Self {
        Self {
            new_block_headers: broadcast::channel(10000).0,
            new_block_with_tx: broadcast::channel(10000).0,
            new_block_state_update: broadcast::channel(10000).0,
            new_mempool_tx: broadcast::channel(10000).0,
            market_events: broadcast::channel(10000).0,
            mempool_events: broadcast::channel(10000).0,
            tx_compose: broadcast::channel(10000).0,
            pool_health_monitor: broadcast::channel(10000).0,
            influxdb_write: None,
            tasks: broadcast::channel(100).0,
        }
    }
}

impl<N: NodePrimitives> EventChannels<N> {
    pub fn with_influxdb(mut self) -> Self {
        self.influxdb_write = Some(broadcast::channel(1000).0);
        self
    }
}
