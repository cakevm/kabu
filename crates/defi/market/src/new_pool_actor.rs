use alloy_network::Network;
use alloy_provider::Provider;
use eyre::Result;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use kabu_core_components::Component;
use kabu_types_events::LoomTask;
use kabu_types_market::PoolLoaders;
use reth_chain_state::{CanonStateNotification, CanonStateSubscriptions};
use reth_ethereum_primitives::EthPrimitives;
use reth_tasks::TaskExecutor;

use crate::logs_parser::process_log_entries;

pub async fn new_pool_worker<P, N>(client: P, pools_loaders: Arc<PoolLoaders<P, N>>, tasks_tx: broadcast::Sender<LoomTask>)
where
    N: Network,
    P: Provider<N> + CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
{
    // Subscribe to canonical state notifications
    let mut stream = client.canonical_state_stream();

    info!("New pool loader subscribed to canonical state notifications");

    while let Some(notification) = stream.next().await {
        match notification {
            CanonStateNotification::Reorg { old: _, new } => {
                debug!("Processing reorg with new chain for pool discovery");
                process_canonical_chain(new, &pools_loaders, &tasks_tx).await;
            }
            CanonStateNotification::Commit { new } => {
                debug!("Processing committed chain for pool discovery");
                process_canonical_chain(new, &pools_loaders, &tasks_tx).await;
            }
        }
    }
}

async fn process_canonical_chain<P, N>(
    chain: Arc<reth_execution_types::Chain<EthPrimitives>>,
    pools_loaders: &Arc<PoolLoaders<P, N>>,
    tasks_tx: &broadcast::Sender<LoomTask>,
) where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    // Get receipts from the chain's execution outcome
    let receipts = chain.execution_outcome().receipts();

    // Collect all logs from all receipts
    let mut all_logs = Vec::new();
    for receipt_vec in receipts {
        for receipt in receipt_vec {
            all_logs.extend(receipt.logs.clone());
        }
    }

    if !all_logs.is_empty() {
        debug!("Processing {} logs for new pool discovery", all_logs.len());
        if let Err(e) = process_log_entries(all_logs, pools_loaders, tasks_tx.clone()).await {
            error!("Error processing logs for new pools: {}", e);
        }
    }
}

pub struct NewPoolLoaderComponent<P, N>
where
    N: Network,
    P: Provider<N> + CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
{
    client: P,
    pool_loaders: Arc<PoolLoaders<P, N>>,
    tasks_tx: Option<broadcast::Sender<LoomTask>>,
}

impl<P, N> NewPoolLoaderComponent<P, N>
where
    N: Network,
    P: Provider<N> + CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
{
    pub fn new(client: P, pool_loaders: Arc<PoolLoaders<P, N>>) -> Self {
        NewPoolLoaderComponent { client, pool_loaders, tasks_tx: None }
    }

    pub fn with_task_channel(self, tasks_tx: broadcast::Sender<LoomTask>) -> Self {
        Self { tasks_tx: Some(tasks_tx), ..self }
    }
}

impl<P, N> Component for NewPoolLoaderComponent<P, N>
where
    N: Network,
    P: Provider<N> + CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let tasks_tx = self.tasks_tx.ok_or_else(|| eyre::eyre!("tasks_tx not set"))?;

        executor.spawn_critical(name, new_pool_worker(self.client, self.pool_loaders, tasks_tx));

        Ok(())
    }
    fn name(&self) -> &'static str {
        "NewPoolLoaderComponent"
    }
}
