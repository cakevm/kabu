use alloy_network::Network;
use alloy_provider::Provider;
use eyre::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error};

use kabu_core_components::Component;
use kabu_types_events::{LoomTask, MessageBlockLogs};
use kabu_types_market::PoolLoaders;
use reth_tasks::TaskExecutor;

use crate::logs_parser::process_log_entries;

pub async fn new_pool_worker<P, N>(
    mut log_update_rx: broadcast::Receiver<MessageBlockLogs>,
    pools_loaders: Arc<PoolLoaders<P, N>>,
    tasks_tx: broadcast::Sender<LoomTask>,
) where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    loop {
        tokio::select! {
            msg = log_update_rx.recv() => {
                debug!("Log update");

                let log_update : Result<MessageBlockLogs, RecvError>  = msg;
                match log_update {
                    Ok(log_update_msg)=>{
                        process_log_entries(
                                log_update_msg.inner.logs,
                                &pools_loaders,
                                tasks_tx.clone(),
                        ).await.unwrap()
                    }
                    Err(e)=>{
                        error!("block_update error {}", e)
                    }
                }

            }
        }
    }
}

pub struct NewPoolLoaderComponent<P, N>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    pool_loaders: Arc<PoolLoaders<P, N>>,
    log_update_rx: Option<broadcast::Sender<MessageBlockLogs>>,
    tasks_tx: Option<broadcast::Sender<LoomTask>>,
}

impl<P, N> NewPoolLoaderComponent<P, N>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    pub fn new(pool_loaders: Arc<PoolLoaders<P, N>>) -> Self {
        NewPoolLoaderComponent { log_update_rx: None, pool_loaders, tasks_tx: None }
    }

    pub fn with_channels(self, log_update_rx: broadcast::Sender<MessageBlockLogs>, tasks_tx: broadcast::Sender<LoomTask>) -> Self {
        Self { log_update_rx: Some(log_update_rx), tasks_tx: Some(tasks_tx), ..self }
    }
}

impl<P, N> Component for NewPoolLoaderComponent<P, N>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let log_update_rx = self.log_update_rx.ok_or_else(|| eyre::eyre!("log_update_rx not set"))?.subscribe();
        let tasks_tx = self.tasks_tx.ok_or_else(|| eyre::eyre!("tasks_tx not set"))?;

        executor.spawn_critical(name, new_pool_worker(log_update_rx, self.pool_loaders, tasks_tx));

        Ok(())
    }
    fn name(&self) -> &'static str {
        "NewPoolLoaderComponent"
    }
}
