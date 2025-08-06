use eyre::Result;
use kabu_core_components::Component;
use kabu_types_blockchain::{ChainParameters, KabuDataTypes, Mempool};
use kabu_types_events::{MempoolEvents, MessageBlock, MessageBlockHeader, MessageMempoolDataUpdate};
use reth_tasks::TaskExecutor;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info};

pub struct MempoolComponent<LDT: KabuDataTypes> {
    chain_parameters: ChainParameters,
    mempool: Arc<RwLock<Mempool<LDT>>>,
    mempool_update_rx: broadcast::Receiver<MessageMempoolDataUpdate<LDT>>,
    block_header_rx: broadcast::Receiver<MessageBlockHeader<LDT>>,
    block_with_txs_rx: broadcast::Receiver<MessageBlock<LDT>>,
    mempool_events_tx: broadcast::Sender<MempoolEvents>,
    influxdb_tx: Option<broadcast::Sender<influxdb::WriteQuery>>,
}

impl<LDT: KabuDataTypes> MempoolComponent<LDT> {
    pub fn new(
        chain_parameters: ChainParameters,
        mempool: Arc<RwLock<Mempool<LDT>>>,
        mempool_update_rx: broadcast::Receiver<MessageMempoolDataUpdate<LDT>>,
        block_header_rx: broadcast::Receiver<MessageBlockHeader<LDT>>,
        block_with_txs_rx: broadcast::Receiver<MessageBlock<LDT>>,
        mempool_events_tx: broadcast::Sender<MempoolEvents>,
        influxdb_tx: Option<broadcast::Sender<influxdb::WriteQuery>>,
    ) -> Self {
        Self { chain_parameters, mempool, mempool_update_rx, block_header_rx, block_with_txs_rx, mempool_events_tx, influxdb_tx }
    }
}

impl<LDT: KabuDataTypes + 'static> Component for MempoolComponent<LDT> {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let mut mempool_update_rx = self.mempool_update_rx;
        let mut block_header_rx = self.block_header_rx;
        let mut block_with_txs_rx = self.block_with_txs_rx;
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
                    Ok(_msg) = block_header_rx.recv() => {
                        // Process new block header
                        let mempool_guard = mempool.write().await;
                        // Remove confirmed transactions
                        drop(mempool_guard);
                    }
                    Ok(_msg) = block_with_txs_rx.recv() => {
                        // Process block with transactions
                        let mempool_guard = mempool.write().await;
                        // Update mempool state
                        drop(mempool_guard);
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
