use crate::blockchain_tokens::add_default_tokens_to_market;
use alloy::primitives::ChainId;
use influxdb::WriteQuery;
use kabu_types_blockchain::{ChainParameters, Mempool};
use kabu_types_entities::AccountNonceAndBalanceState;
use kabu_types_events::{
    LoomTask, MarketEvents, MempoolEvents, MessageBlock, MessageBlockHeader, MessageBlockStateUpdate, MessageHealthEvent,
    MessageMempoolDataUpdate, MessageTxCompose,
};
use kabu_types_market::Market;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::error;

#[derive(Clone)]
pub struct Blockchain<N: NodePrimitives = EthPrimitives> {
    chain_id: ChainId,
    chain_parameters: ChainParameters,
    market: Arc<RwLock<Market>>,
    mempool: Arc<RwLock<Mempool<N>>>,
    account_nonce_and_balance: Arc<RwLock<AccountNonceAndBalanceState>>,

    new_block_headers_channel: broadcast::Sender<MessageBlockHeader<N>>,
    new_block_with_tx_channel: broadcast::Sender<MessageBlock<N>>,
    new_block_state_update_channel: broadcast::Sender<MessageBlockStateUpdate<N>>,
    new_mempool_tx_channel: broadcast::Sender<MessageMempoolDataUpdate<N>>,
    market_events_channel: broadcast::Sender<MarketEvents>,
    mempool_events_channel: broadcast::Sender<MempoolEvents>,
    tx_compose_channel: broadcast::Sender<MessageTxCompose<N>>,

    pool_health_monitor_channel: broadcast::Sender<MessageHealthEvent>,
    influxdb_write_channel: Option<broadcast::Sender<WriteQuery>>,
    tasks_channel: broadcast::Sender<LoomTask>,
}

impl Blockchain<EthPrimitives> {
    pub fn new(chain_id: ChainId) -> Blockchain<EthPrimitives> {
        Self::new_with_config(chain_id, true)
    }

    pub fn new_with_config(chain_id: ChainId, enable_influxdb: bool) -> Blockchain<EthPrimitives> {
        let new_block_headers_channel: broadcast::Sender<MessageBlockHeader<EthPrimitives>> = broadcast::channel(10).0;
        let new_block_with_tx_channel: broadcast::Sender<MessageBlock<EthPrimitives>> = broadcast::channel(10).0;
        let new_block_state_update_channel: broadcast::Sender<MessageBlockStateUpdate<EthPrimitives>> = broadcast::channel(10).0;

        let new_mempool_tx_channel: broadcast::Sender<MessageMempoolDataUpdate<EthPrimitives>> = broadcast::channel(5000).0;

        let market_events_channel: broadcast::Sender<MarketEvents> = broadcast::channel(100).0;
        let mempool_events_channel: broadcast::Sender<MempoolEvents> = broadcast::channel(2000).0;
        let tx_compose_channel: broadcast::Sender<MessageTxCompose<EthPrimitives>> = broadcast::channel(2000).0;

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
            mempool: Arc::new(RwLock::new(Mempool::<EthPrimitives>::new())),
            account_nonce_and_balance: Arc::new(RwLock::new(AccountNonceAndBalanceState::new())),
            new_block_headers_channel,
            new_block_with_tx_channel,
            new_block_state_update_channel,
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

impl<N: NodePrimitives> Blockchain<N> {
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn chain_parameters(&self) -> ChainParameters {
        self.chain_parameters.clone()
    }

    pub fn market(&self) -> Arc<RwLock<Market>> {
        self.market.clone()
    }

    pub fn mempool(&self) -> Arc<RwLock<Mempool<N>>> {
        self.mempool.clone()
    }

    pub fn nonce_and_balance(&self) -> Arc<RwLock<AccountNonceAndBalanceState>> {
        self.account_nonce_and_balance.clone()
    }

    pub fn new_block_headers_channel(&self) -> broadcast::Sender<MessageBlockHeader<N>> {
        self.new_block_headers_channel.clone()
    }

    pub fn new_block_with_tx_channel(&self) -> broadcast::Sender<MessageBlock<N>> {
        self.new_block_with_tx_channel.clone()
    }

    pub fn new_block_state_update_channel(&self) -> broadcast::Sender<MessageBlockStateUpdate<N>> {
        self.new_block_state_update_channel.clone()
    }

    pub fn new_mempool_tx_channel(&self) -> broadcast::Sender<MessageMempoolDataUpdate<N>> {
        self.new_mempool_tx_channel.clone()
    }

    pub fn market_events_channel(&self) -> broadcast::Sender<MarketEvents> {
        self.market_events_channel.clone()
    }

    pub fn mempool_events_channel(&self) -> broadcast::Sender<MempoolEvents> {
        self.mempool_events_channel.clone()
    }

    pub fn tx_compose_channel(&self) -> broadcast::Sender<MessageTxCompose<N>> {
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
