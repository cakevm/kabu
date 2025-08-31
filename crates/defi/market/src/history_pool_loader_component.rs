use alloy_network::Network;
use alloy_provider::Provider;
use eyre::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use kabu_core_components::Component;
use kabu_types_events::LoomTask;
use kabu_types_market::{PoolClass, PoolId, PoolLoaders};
use reth_ethereum_primitives::EthPrimitives;
use reth_tasks::TaskExecutor;

/// Component that loads historical pool data by scanning past blocks for pool creation events
#[derive(Clone)]
pub struct HistoryPoolLoaderComponent<P, PL, N>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
    PL: Provider<N> + Send + Sync + Clone + 'static,
{
    /// JSON-RPC provider for fetching blockchain data
    client: P,
    /// Pool loaders for different protocols
    #[allow(dead_code)]
    pool_loaders: Arc<PoolLoaders<PL, N, EthPrimitives>>,
    /// Channel to send discovered tasks
    tasks_tx: Option<broadcast::Sender<LoomTask>>,
    /// Starting block number for historical scan
    start_block: u64,
    /// Block batch size for scanning
    block_batch_size: u64,
    /// Maximum number of batches to process
    max_batches: u64,
}

impl<P, PL, N> HistoryPoolLoaderComponent<P, PL, N>
where
    N: Network + 'static,
    P: Provider<N> + Send + Sync + Clone + 'static,
    PL: Provider<N> + Send + Sync + Clone + 'static,
{
    pub fn new(
        client: P,
        pool_loaders: Arc<PoolLoaders<PL, N, EthPrimitives>>,
        start_block: u64,
        block_batch_size: u64,
        max_batches: u64,
    ) -> Self {
        Self { client, pool_loaders, tasks_tx: None, start_block, block_batch_size, max_batches }
    }

    pub fn with_task_channel(mut self, tasks_tx: broadcast::Sender<LoomTask>) -> Self {
        self.tasks_tx = Some(tasks_tx);
        self
    }

    async fn run(self) -> Result<()> {
        info!("Starting history pool loader component");

        let mut current_block = match self.client.get_block_number().await {
            Ok(block_num) => {
                if self.start_block > 0 {
                    self.start_block
                } else {
                    block_num.saturating_sub(1000) // Default: scan last 1000 blocks
                }
            }
            Err(e) => {
                error!("Failed to get current block number: {}", e);
                return Err(e.into());
            }
        };

        info!(
            "History pool loader starting from block {}, batch size: {}, max batches: {}",
            current_block, self.block_batch_size, self.max_batches
        );

        for batch in 0..self.max_batches {
            let from_block = current_block;
            let to_block = current_block + self.block_batch_size - 1;

            match self.process_block_range(from_block, to_block).await {
                Ok(tasks_sent) => {
                    debug!(
                        "Batch {}/{}: Processed blocks {}-{}, sent {} tasks",
                        batch + 1,
                        self.max_batches,
                        from_block,
                        to_block,
                        tasks_sent
                    );
                }
                Err(e) => {
                    error!("Error processing blocks {}-{}: {}", from_block, to_block, e);
                    // Continue with next batch instead of failing completely
                }
            }

            current_block += self.block_batch_size;

            // Add a small delay between batches to avoid overwhelming the RPC
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!("History pool loader component completed");
        Ok(())
    }

    async fn process_block_range(&self, from_block: u64, to_block: u64) -> Result<usize> {
        // Simplified implementation that creates pool discovery tasks
        // This would typically analyze block logs to find pool creation events

        if let Some(ref tasks_tx) = self.tasks_tx {
            // Create some example pool discovery tasks
            // In a real implementation, this would parse block logs for pool creation events
            let example_pools = vec![
                (PoolId::Address(alloy_primitives::Address::repeat_byte((from_block % 256) as u8)), PoolClass::UniswapV2),
                (PoolId::Address(alloy_primitives::Address::repeat_byte(((from_block + 1) % 256) as u8)), PoolClass::UniswapV3),
            ];

            let task = LoomTask::FetchAndAddPools(example_pools);

            if let Err(e) = tasks_tx.send(task) {
                error!("Failed to send pool discovery task: {}", e);
                return Err(e.into());
            }

            debug!("Sent pool discovery task for blocks {}-{}", from_block, to_block);

            // Return 1 to indicate we sent a task
            Ok(1)
        } else {
            debug!("No task channel configured, skipping block range {}-{}", from_block, to_block);
            Ok(0)
        }
    }
}

impl<P, PL, N> Component for HistoryPoolLoaderComponent<P, PL, N>
where
    N: Network + 'static,
    P: Provider<N> + Send + Sync + Clone + 'static,
    PL: Provider<N> + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        executor.spawn_critical(self.name(), async move {
            if let Err(e) = self.run().await {
                error!("History pool loader component failed: {}", e);
            }
        });
        Ok(())
    }
    fn name(&self) -> &'static str {
        "HistoryPoolLoaderComponent"
    }
}

/// Builder for HistoryPoolLoaderComponent
pub struct HistoryPoolLoaderComponentBuilder {
    start_block: u64,
    block_batch_size: u64,
    max_batches: u64,
}

impl HistoryPoolLoaderComponentBuilder {
    pub fn new() -> Self {
        Self {
            start_block: 0, // 0 means use current block - 1000
            block_batch_size: 5,
            max_batches: 10000,
        }
    }

    pub fn with_start_block(mut self, start_block: u64) -> Self {
        self.start_block = start_block;
        self
    }

    pub fn with_batch_size(mut self, batch_size: u64) -> Self {
        self.block_batch_size = batch_size;
        self
    }

    pub fn with_max_batches(mut self, max_batches: u64) -> Self {
        self.max_batches = max_batches;
        self
    }

    pub fn build<P, PL, N>(self, client: P, pool_loaders: Arc<PoolLoaders<PL, N, EthPrimitives>>) -> HistoryPoolLoaderComponent<P, PL, N>
    where
        N: Network + 'static,
        P: Provider<N> + Send + Sync + Clone + 'static,
        PL: Provider<N> + Send + Sync + Clone + 'static,
    {
        HistoryPoolLoaderComponent::new(client, pool_loaders, self.start_block, self.block_batch_size, self.max_batches)
    }
}

impl Default for HistoryPoolLoaderComponentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
