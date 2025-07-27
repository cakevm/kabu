use alloy::primitives::ChainId;
use kabu_types_blockchain::ChainParameters;
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum, Mempool};
use kabu_types_entities::{AccountNonceAndBalanceState, LatestBlock};
use kabu_types_market::Market;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Clone)]
pub struct AppState<LDT: KabuDataTypes + 'static = KabuDataTypesEthereum> {
    pub chain_id: ChainId,
    pub chain_parameters: ChainParameters,
    pub market: Arc<RwLock<Market>>,
    pub latest_block: Arc<RwLock<LatestBlock<LDT>>>,
    pub mempool: Arc<RwLock<Mempool<LDT>>>,
    pub account_nonce_and_balance: Arc<RwLock<AccountNonceAndBalanceState>>,
}

impl<LDT: KabuDataTypes + 'static> AppState<LDT>
where
    LDT: Default,
{
    pub fn new(chain_id: ChainId) -> Self {
        let chain_parameters = ChainParameters::ethereum();

        Self {
            chain_id,
            chain_parameters,
            market: Arc::new(RwLock::new(Market::default())),
            latest_block: Arc::new(RwLock::new(LatestBlock::new(0, Default::default()))),
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
pub struct EventChannels<LDT: KabuDataTypes + 'static = KabuDataTypesEthereum> {
    pub new_block_headers: broadcast::Sender<kabu_types_events::MessageBlockHeader<LDT>>,
    pub new_block_with_tx: broadcast::Sender<kabu_types_events::MessageBlock<LDT>>,
    pub new_block_state_update: broadcast::Sender<kabu_types_events::MessageBlockStateUpdate<LDT>>,
    pub new_block_logs: broadcast::Sender<kabu_types_events::MessageBlockLogs<LDT>>,
    pub new_mempool_tx: broadcast::Sender<kabu_types_events::MessageMempoolDataUpdate<LDT>>,
    pub market_events: broadcast::Sender<kabu_types_events::MarketEvents>,
    pub mempool_events: broadcast::Sender<kabu_types_events::MempoolEvents>,
    pub tx_compose: broadcast::Sender<kabu_types_events::MessageTxCompose<LDT>>,
    pub pool_health_monitor: broadcast::Sender<kabu_types_events::MessageHealthEvent>,
    pub influxdb_write: Option<broadcast::Sender<influxdb::WriteQuery>>,
    pub tasks: broadcast::Sender<kabu_types_events::LoomTask>,
}

impl<LDT: KabuDataTypes + 'static> Default for EventChannels<LDT> {
    fn default() -> Self {
        Self {
            new_block_headers: broadcast::channel(10000).0,
            new_block_with_tx: broadcast::channel(10000).0,
            new_block_state_update: broadcast::channel(10000).0,
            new_block_logs: broadcast::channel(10000).0,
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

impl<LDT: KabuDataTypes + 'static> EventChannels<LDT> {
    pub fn with_influxdb(mut self) -> Self {
        self.influxdb_write = Some(broadcast::channel(1000).0);
        self
    }
}
