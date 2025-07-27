use eyre::{eyre, Result};
use influxdb::{Timestamp, WriteQuery};
use kabu_core_components::Component;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_evm_db::DatabaseKabuExt;
use kabu_types_blockchain::KabuDataTypes;
use kabu_types_events::MessageBlockHeader;
use kabu_types_market::{Market, MarketState};
use reth_tasks::TaskExecutor;
use revm::DatabaseRef;
use std::time::Duration;
use tikv_jemalloc_ctl::stats;
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info};

async fn metrics_recorder_worker<DB: DatabaseKabuExt + DatabaseRef + Send + Sync + 'static, LDT: KabuDataTypes>(
    market: Arc<RwLock<Market>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    block_header_update_rx: broadcast::Sender<MessageBlockHeader<LDT>>,
    influx_channel_tx: Option<broadcast::Sender<WriteQuery>>,
) -> Result<()> {
    let mut block_header_update_receiver = block_header_update_rx.subscribe();
    loop {
        let block_header = match block_header_update_receiver.recv().await {
            Ok(block) => block,
            Err(e) => match e {
                RecvError::Closed => {
                    error!("Block header channel closed");
                    return Err(eyre!("Block header channel closed".to_string()));
                }
                RecvError::Lagged(lag) => {
                    info!("Block header channel lagged: {}", lag);
                    continue;
                }
            },
        };

        let current_timestamp = chrono::Utc::now();
        let block_latency = current_timestamp.timestamp() as f64 - block_header.inner.header.timestamp as f64;

        // check if we received twice the same block number

        let allocated = stats::allocated::read().unwrap_or_default();

        let market_state_guard = market_state.read().await;
        let accounts = market_state_guard.state_db.accounts_len();
        let contracts = market_state_guard.state_db.contracts_len();

        drop(market_state_guard);

        let market_guard = market.read().await;
        let pools_disabled = market_guard.disabled_pools_count();
        let paths = market_guard.swap_paths().len();
        let paths_disabled = market_guard.swap_paths().disabled_len();
        drop(market_guard);

        let influx_channel_clone = influx_channel_tx.clone();

        let block_number = block_header.inner.header.number;

        if let Some(influx_tx) = influx_channel_clone {
            if let Err(e) = tokio::time::timeout(Duration::from_secs(2), async move {
                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "state_accounts")
                    .add_field("value", accounts as f32)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send block latency to influxdb: {:?}", e);
                }

                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "state_contracts")
                    .add_field("value", contracts as f32)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send block latency to influxdb: {:?}", e);
                }

                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "pools_disabled")
                    .add_field("value", pools_disabled as f32)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send pools_disabled to influxdb: {:?}", e);
                }

                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "paths")
                    .add_field("value", paths as f32)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send pools_disabled to influxdb: {:?}", e);
                }

                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "paths_disabled")
                    .add_field("value", paths_disabled as f32)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send pools_disabled to influxdb: {:?}", e);
                }

                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "pools_disabled")
                    .add_field("value", pools_disabled as f32)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send pools_disabled to influxdb: {:?}", e);
                }

                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "jemalloc_allocated")
                    .add_field("value", (allocated >> 20) as f32)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send jemalloc_allocator latency to influxdb: {:?}", e);
                }

                let write_query = WriteQuery::new(Timestamp::from(current_timestamp), "block_latency")
                    .add_field("value", block_latency)
                    .add_field("block_number", block_number);
                if let Err(e) = influx_tx.send(write_query) {
                    error!("Failed to send block latency to influxdb: {:?}", e);
                }
            })
            .await
            {
                error!("Failed to send data to influxdb: {:?}", e);
            }
        }
    }
}

pub struct MetricsRecorderActor<DB: Clone + Send + Sync + 'static, LDT: KabuDataTypes + 'static> {
    market: Option<Arc<RwLock<Market>>>,

    market_state: Option<Arc<RwLock<MarketState<DB>>>>,

    block_header_rx: Option<broadcast::Sender<MessageBlockHeader<LDT>>>,

    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
}

impl<DB, LDT> Default for MetricsRecorderActor<DB, LDT>
where
    DB: DatabaseRef + DatabaseKabuExt + Clone + Send + Sync + 'static,
    LDT: KabuDataTypes + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<DB, LDT> MetricsRecorderActor<DB, LDT>
where
    DB: DatabaseRef + DatabaseKabuExt + Clone + Send + Sync + 'static,
    LDT: KabuDataTypes + 'static,
{
    pub fn new() -> Self {
        Self { market: None, market_state: None, block_header_rx: None, influxdb_write_channel_tx: None }
    }

    pub fn on_bc(self, bc: &Blockchain<LDT>, bc_state: &BlockchainState<DB, LDT>) -> Self {
        Self {
            market: Some(bc.market()),
            market_state: Some(bc_state.market_state()),
            block_header_rx: Some(bc.new_block_headers_channel()),
            influxdb_write_channel_tx: bc.influxdb_write_channel(),
        }
    }
}

impl<DB, LDT> Component for MetricsRecorderActor<DB, LDT>
where
    DB: DatabaseRef + DatabaseKabuExt + Clone + Send + Sync + 'static,
    LDT: KabuDataTypes + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let market = self.market.ok_or_else(|| eyre!("market not set"))?;
        let market_state = self.market_state.ok_or_else(|| eyre!("market_state not set"))?;
        let block_header_rx = self.block_header_rx.ok_or_else(|| eyre!("block_header_rx not set"))?;

        executor.spawn_critical(name, async move {
            if let Err(e) = metrics_recorder_worker(market, market_state, block_header_rx, self.influxdb_write_channel_tx).await {
                tracing::error!("Metrics recorder worker failed: {}", e);
            }
        });

        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "BlockLatencyRecorderActor"
    }
}
