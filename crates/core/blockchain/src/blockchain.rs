use crate::blockchain_tokens::add_default_tokens_to_market;
use alloy::primitives::BlockHash;
use alloy::primitives::ChainId;
use influxdb::WriteQuery;
use kabu_types_blockchain::{ChainParameters, Mempool};
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_entities::{AccountNonceAndBalanceState, LatestBlock};
use kabu_types_events::{
    LoomTask, MarketEvents, MempoolEvents, MessageBlock, MessageBlockHeader, MessageBlockLogs, MessageBlockStateUpdate, MessageHealthEvent,
    MessageMempoolDataUpdate, MessageTxCompose,
};
use kabu_types_market::Market;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::error;

#[derive(Clone)]
pub struct Blockchain<LDT: KabuDataTypes + 'static = KabuDataTypesEthereum> {
    chain_id: ChainId,
    chain_parameters: ChainParameters,
    market: Arc<RwLock<Market>>,
    latest_block: Arc<RwLock<LatestBlock<LDT>>>,
    mempool: Arc<RwLock<Mempool<LDT>>>,
    account_nonce_and_balance: Arc<RwLock<AccountNonceAndBalanceState>>,

    new_block_headers_channel: broadcast::Sender<MessageBlockHeader<LDT>>,
    new_block_with_tx_channel: broadcast::Sender<MessageBlock<LDT>>,
    new_block_state_update_channel: broadcast::Sender<MessageBlockStateUpdate<LDT>>,
    new_block_logs_channel: broadcast::Sender<MessageBlockLogs<LDT>>,
    new_mempool_tx_channel: broadcast::Sender<MessageMempoolDataUpdate<LDT>>,
    market_events_channel: broadcast::Sender<MarketEvents>,
    mempool_events_channel: broadcast::Sender<MempoolEvents>,
    tx_compose_channel: broadcast::Sender<MessageTxCompose<LDT>>,

    pool_health_monitor_channel: broadcast::Sender<MessageHealthEvent>,
    influxdb_write_channel: Option<broadcast::Sender<WriteQuery>>,
    tasks_channel: broadcast::Sender<LoomTask>,
}

impl Blockchain<KabuDataTypesEthereum> {
    pub fn new(chain_id: ChainId) -> Blockchain<KabuDataTypesEthereum> {
        Self::new_with_config(chain_id, true)
    }

    pub fn new_with_config(chain_id: ChainId, enable_influxdb: bool) -> Blockchain<KabuDataTypesEthereum> {
        let new_block_headers_channel: broadcast::Sender<MessageBlockHeader> = broadcast::channel(10).0;
        let new_block_with_tx_channel: broadcast::Sender<MessageBlock> = broadcast::channel(10).0;
        let new_block_state_update_channel: broadcast::Sender<MessageBlockStateUpdate> = broadcast::channel(10).0;
        let new_block_logs_channel: broadcast::Sender<MessageBlockLogs> = broadcast::channel(10).0;

        let new_mempool_tx_channel: broadcast::Sender<MessageMempoolDataUpdate> = broadcast::channel(5000).0;

        let market_events_channel: broadcast::Sender<MarketEvents> = broadcast::channel(100).0;
        let mempool_events_channel: broadcast::Sender<MempoolEvents> = broadcast::channel(2000).0;
        let tx_compose_channel: broadcast::Sender<MessageTxCompose> = broadcast::channel(2000).0;

        let pool_health_monitor_channel: broadcast::Sender<MessageHealthEvent> = broadcast::channel(1000).0;
        let influx_write_channel = if enable_influxdb { Some(broadcast::channel(1000).0) } else { None };
        let tasks_channel: broadcast::Sender<LoomTask> = broadcast::channel(1000).0;

        let mut market_instance = Market::default();

        if let Err(error) = add_default_tokens_to_market(&mut market_instance, chain_id) {
            error!(%error, "Failed to add default tokens to market");
        }

        Blockchain {
            chain_id,
            chain_parameters: ChainParameters::ethereum(),
            market: Arc::new(RwLock::new(market_instance)),
            mempool: Arc::new(RwLock::new(Mempool::<KabuDataTypesEthereum>::new())),
            latest_block: Arc::new(RwLock::new(LatestBlock::new(0, BlockHash::ZERO))),
            account_nonce_and_balance: Arc::new(RwLock::new(AccountNonceAndBalanceState::new())),
            new_block_headers_channel,
            new_block_with_tx_channel,
            new_block_state_update_channel,
            new_block_logs_channel,
            new_mempool_tx_channel,
            market_events_channel,
            mempool_events_channel,
            pool_health_monitor_channel,
            tx_compose_channel,
            influxdb_write_channel: influx_write_channel,
            tasks_channel,
        }
    }
}

impl<LDT: KabuDataTypes> Blockchain<LDT> {
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn chain_parameters(&self) -> ChainParameters {
        self.chain_parameters.clone()
    }

    pub fn market(&self) -> Arc<RwLock<Market>> {
        self.market.clone()
    }

    pub fn latest_block(&self) -> Arc<RwLock<LatestBlock<LDT>>> {
        self.latest_block.clone()
    }

    pub fn mempool(&self) -> Arc<RwLock<Mempool<LDT>>> {
        self.mempool.clone()
    }

    pub fn nonce_and_balance(&self) -> Arc<RwLock<AccountNonceAndBalanceState>> {
        self.account_nonce_and_balance.clone()
    }

    pub fn new_block_headers_channel(&self) -> broadcast::Sender<MessageBlockHeader<LDT>> {
        self.new_block_headers_channel.clone()
    }

    pub fn new_block_with_tx_channel(&self) -> broadcast::Sender<MessageBlock<LDT>> {
        self.new_block_with_tx_channel.clone()
    }

    pub fn new_block_state_update_channel(&self) -> broadcast::Sender<MessageBlockStateUpdate<LDT>> {
        self.new_block_state_update_channel.clone()
    }

    pub fn new_block_logs_channel(&self) -> broadcast::Sender<MessageBlockLogs<LDT>> {
        self.new_block_logs_channel.clone()
    }

    pub fn new_mempool_tx_channel(&self) -> broadcast::Sender<MessageMempoolDataUpdate<LDT>> {
        self.new_mempool_tx_channel.clone()
    }

    pub fn market_events_channel(&self) -> broadcast::Sender<MarketEvents> {
        self.market_events_channel.clone()
    }

    pub fn mempool_events_channel(&self) -> broadcast::Sender<MempoolEvents> {
        self.mempool_events_channel.clone()
    }

    pub fn tx_compose_channel(&self) -> broadcast::Sender<MessageTxCompose<LDT>> {
        self.tx_compose_channel.clone()
    }

    pub fn health_monitor_channel(&self) -> broadcast::Sender<MessageHealthEvent> {
        self.pool_health_monitor_channel.clone()
    }

    pub fn influxdb_write_channel(&self) -> Option<broadcast::Sender<WriteQuery>> {
        self.influxdb_write_channel.clone()
    }

    pub fn tasks_channel(&self) -> broadcast::Sender<LoomTask> {
        self.tasks_channel.clone()
    }
}
