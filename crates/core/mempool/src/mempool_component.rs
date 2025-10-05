use eyre::Result;
use kabu_core_components::Component;
use kabu_types_blockchain::{ChainParameters, Mempool};
use kabu_types_events::{MarketEvents, MempoolEvents, MessageMempoolDataUpdate};
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use reth_tasks::TaskExecutor;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{error, info};

pub struct MempoolComponent<N: NodePrimitives = EthPrimitives> {
    chain_parameters: ChainParameters,
    mempool: Arc<RwLock<Mempool<N>>>,
    mempool_update_rx: broadcast::Receiver<MessageMempoolDataUpdate<N>>,
    market_events_rx: broadcast::Receiver<MarketEvents>,
    mempool_events_tx: broadcast::Sender<MempoolEvents>,
    influxdb_tx: Option<broadcast::Sender<influxdb::WriteQuery>>,
}

impl<N: NodePrimitives> MempoolComponent<N> {
    pub fn new(
        chain_parameters: ChainParameters,
        mempool: Arc<RwLock<Mempool<N>>>,
        mempool_update_rx: broadcast::Receiver<MessageMempoolDataUpdate<N>>,
        market_events_rx: broadcast::Receiver<MarketEvents>,
        mempool_events_tx: broadcast::Sender<MempoolEvents>,
        influxdb_tx: Option<broadcast::Sender<influxdb::WriteQuery>>,
    ) -> Self {
        Self { chain_parameters, mempool, mempool_update_rx, market_events_rx, mempool_events_tx, influxdb_tx }
    }
}

impl<N: NodePrimitives + 'static> Component for MempoolComponent<N> {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let mut mempool_update_rx = self.mempool_update_rx;
        let mut market_events_rx = self.market_events_rx;
        let mempool = self.mempool;
        let _mempool_events_tx = self.mempool_events_tx;
        let _influxdb_tx = self.influxdb_tx;
        let _chain_parameters = self.chain_parameters;

        // Spawn the main mempool task
        executor.spawn_critical(name, async move {
            info!("Starting mempool component");

            loop {
                tokio::select! {
                    Ok(_msg) = mempool_update_rx.recv() => {
                        // Process mempool update
                        let mempool_guard = mempool.write().await;
                        // Add transaction processing logic here
                        drop(mempool_guard);
                    }
                    Ok(event) = market_events_rx.recv() => {
                        match event {
                            MarketEvents::BlockHeaderUpdate { .. } => {
                                // Process new block header
                                let mempool_guard = mempool.write().await;
                                // Remove confirmed transactions based on block number
                                drop(mempool_guard);
                            }
                            MarketEvents::BlockTxUpdate { .. } => {
                                // Process block with transactions
                                let mempool_guard = mempool.write().await;
                                // Update mempool state based on block transactions
                                drop(mempool_guard);
                            }
                            _ => {
                                // Ignore other market events
                            }
                        }
                    }
                    else => {
                        error!("All channels closed, stopping mempool component");
                        break;
                    }
                }
            }
        });

        Ok(())
    }
    fn name(&self) -> &'static str {
        "MempoolComponent"
    }
}
