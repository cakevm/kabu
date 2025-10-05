use eyre::{Result, eyre};
use influxdb::{Timestamp, WriteQuery};
use kabu_core_components::Component;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_evm_db::DatabaseKabuExt;
use kabu_types_events::MarketEvents;
use kabu_types_market::{Market, MarketState};
use reth_node_types::NodePrimitives;
use reth_tasks::TaskExecutor;
use revm::DatabaseRef;
use std::time::Duration;
use tikv_jemalloc_ctl::stats;
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info};

async fn metrics_recorder_worker<DB: DatabaseKabuExt + DatabaseRef + Send + Sync + 'static>(
    market: Arc<RwLock<Market>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    market_events_rx: broadcast::Sender<MarketEvents>,
    influx_channel_tx: Option<broadcast::Sender<WriteQuery>>,
) -> Result<()> {
    let mut market_events_receiver = market_events_rx.subscribe();
    loop {
        let market_event = match market_events_receiver.recv().await {
            Ok(event) => event,
            Err(e) => match e {
                RecvError::Closed => {
                    error!("Market events channel closed");
                    return Err(eyre!("Market events channel closed".to_string()));
                }
                RecvError::Lagged(lag) => {
                    info!("Market events channel lagged: {}", lag);
                    continue;
                }
            },
        };

        // Only process BlockHeaderUpdate events for metrics
        let (block_number, timestamp) = match market_event {
            MarketEvents::BlockHeaderUpdate { block_number, timestamp, .. } => (block_number, timestamp),
            _ => continue, // Skip non-block events
        };

        let current_timestamp = chrono::Utc::now();
        let block_latency = current_timestamp.timestamp() as f64 - timestamp as f64;

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

        if let Some(influx_tx) = influx_channel_clone
            && let Err(e) = tokio::time::timeout(Duration::from_secs(2), async move {
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

pub struct MetricsRecorderActor<DB: Clone + Send + Sync + 'static> {
    market: Option<Arc<RwLock<Market>>>,

    market_state: Option<Arc<RwLock<MarketState<DB>>>>,

    market_events_rx: Option<broadcast::Sender<MarketEvents>>,

    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
}

impl<DB> Default for MetricsRecorderActor<DB>
where
    DB: DatabaseRef + DatabaseKabuExt + Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<DB> MetricsRecorderActor<DB>
where
    DB: DatabaseRef + DatabaseKabuExt + Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self { market: None, market_state: None, market_events_rx: None, influxdb_write_channel_tx: None }
    }

    pub fn on_bc<NP: NodePrimitives>(self, bc: &Blockchain<NP>, bc_state: &BlockchainState<DB, NP>) -> Self {
        Self {
            market: Some(bc.market()),
            market_state: Some(bc_state.market_state()),
            market_events_rx: Some(bc.market_events_channel()),
            influxdb_write_channel_tx: bc.influxdb_write_channel(),
        }
    }
}

impl<DB> Component for MetricsRecorderActor<DB>
where
    DB: DatabaseRef + DatabaseKabuExt + Clone + Send + Sync + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let market = self.market.ok_or_else(|| eyre!("market not set"))?;
        let market_state = self.market_state.ok_or_else(|| eyre!("market_state not set"))?;
        let market_events_rx = self.market_events_rx.ok_or_else(|| eyre!("market_events_rx not set"))?;

        executor.spawn_critical(name, async move {
            if let Err(e) = metrics_recorder_worker(market, market_state, market_events_rx, self.influxdb_write_channel_tx).await {
                tracing::error!("Metrics recorder worker failed: {}", e);
            }
        });

        Ok(())
    }
    fn name(&self) -> &'static str {
        "BlockLatencyRecorderActor"
    }
}
