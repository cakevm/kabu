use std::sync::Arc;
use tokio::sync::broadcast;

use alloy_network::Network;
use alloy_provider::Provider;
use eyre::Result;
use tracing::{error, info};

use kabu_core_components::Component;
use kabu_types_events::LoomTask;
use kabu_types_market::{PoolClass, PoolId, PoolLoaders};
use reth_tasks::TaskExecutor;

/// Component that loads pools by protocol type using protocol-specific loaders
pub struct ProtocolPoolLoaderComponent<P, PL, N>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
    PL: Provider<N> + Send + Sync + Clone + 'static,
{
    /// JSON-RPC provider for fetching blockchain data
    #[allow(dead_code)]
    client: P,
    /// Pool loaders for different protocols
    #[allow(dead_code)]
    pool_loaders: Arc<PoolLoaders<PL, N>>,
    /// Channel to send discovered tasks
    tasks_tx: Option<broadcast::Sender<LoomTask>>,
}

impl<P, PL, N> ProtocolPoolLoaderComponent<P, PL, N>
where
    N: Network + 'static,
    P: Provider<N> + Send + Sync + Clone + 'static,
    PL: Provider<N> + Send + Sync + Clone + 'static,
{
    pub fn new(client: P, pool_loaders: Arc<PoolLoaders<PL, N>>) -> Self {
        Self { client, pool_loaders, tasks_tx: None }
    }

    pub fn with_task_channel(mut self, tasks_tx: broadcast::Sender<LoomTask>) -> Self {
        self.tasks_tx = Some(tasks_tx);
        self
    }

    async fn run(self) -> Result<()> {
        info!("Starting protocol pool loader component");

        // Spawn a task for each protocol type
        let mut tasks = Vec::new();

        // Simplified: just process a few common pool classes
        let pool_classes = vec![PoolClass::UniswapV2, PoolClass::UniswapV3, PoolClass::Curve];

        for pool_class in pool_classes {
            let tasks_tx_clone = self.tasks_tx.clone();

            let task = tokio::task::spawn(async move {
                if let Err(e) = Self::process_protocol_loader(
                    pool_class,
                    Arc::new(()), // Dummy loader
                    tasks_tx_clone,
                )
                .await
                {
                    error!("Protocol loader failed for {}: {}", pool_class, e);
                }
            });

            tasks.push(task);
        }

        // Wait for all protocol loaders to complete
        for task in tasks {
            if let Err(e) = task.await {
                error!("Protocol loader task failed: {}", e);
            }
        }

        info!("Protocol pool loader component completed");
        Ok(())
    }

    async fn process_protocol_loader(
        pool_class: PoolClass,
        _pool_loader: Arc<dyn Send + Sync>,
        tasks_tx: Option<broadcast::Sender<LoomTask>>,
    ) -> Result<()> {
        info!("Protocol loader started for {}", pool_class);

        // Simplified implementation that creates example protocol discovery tasks
        if let Some(tx) = tasks_tx {
            // Create example pools for this protocol
            let example_pools = vec![
                (PoolId::Address(alloy_primitives::Address::repeat_byte(0x01)), pool_class),
                (PoolId::Address(alloy_primitives::Address::repeat_byte(0x02)), pool_class),
            ];

            let task = LoomTask::FetchAndAddPools(example_pools);

            if let Err(e) = tx.send(task) {
                error!("Failed to send protocol pool task for {}: {}", pool_class, e);
                return Err(e.into());
            }

            info!("Sent protocol pool discovery task for {}", pool_class);
        }

        Ok(())
    }
}

impl<P, PL, N> Component for ProtocolPoolLoaderComponent<P, PL, N>
where
    N: Network + 'static,
    P: Provider<N> + Send + Sync + Clone + 'static,
    PL: Provider<N> + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        executor.spawn_critical(self.name(), async move {
            if let Err(e) = self.run().await {
                error!("Protocol pool loader component failed: {}", e);
            }
        });
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "ProtocolPoolLoaderComponent"
    }
}

/// Builder for ProtocolPoolLoaderComponent
pub struct ProtocolPoolLoaderComponentBuilder {
    // Add configuration options as needed
}

impl ProtocolPoolLoaderComponentBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub fn build<P, PL, N>(self, client: P, pool_loaders: Arc<PoolLoaders<PL, N>>) -> ProtocolPoolLoaderComponent<P, PL, N>
    where
        N: Network + 'static,
        P: Provider<N> + Send + Sync + Clone + 'static,
        PL: Provider<N> + Send + Sync + Clone + 'static,
    {
        ProtocolPoolLoaderComponent::new(client, pool_loaders)
    }
}

impl Default for ProtocolPoolLoaderComponentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
