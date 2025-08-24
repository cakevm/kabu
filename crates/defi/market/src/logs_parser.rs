use alloy_network::Network;
use alloy_primitives::Log;
use alloy_provider::Provider;
use eyre::Result;
use std::collections::HashMap;
use tokio::sync::broadcast;
use tracing::error;

use kabu_types_events::LoomTask;
use kabu_types_market::PoolLoaders;

pub async fn process_log_entries<P, N>(
    log_entries: Vec<Log>,
    pool_loaders: &PoolLoaders<P, N>,
    tasks_tx: broadcast::Sender<LoomTask>,
) -> Result<()>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    let mut pool_to_fetch = Vec::new();
    let mut processed_pools = HashMap::new();

    for log_entry in log_entries.into_iter() {
        if let Some((pool_id, pool_class)) = pool_loaders.determine_pool_class(&log_entry) {
            // was this pool already processed?
            if processed_pools.insert(log_entry.address, true).is_some() {
                continue;
            }

            pool_to_fetch.push((pool_id, pool_class));
        }
    }

    if let Err(e) = tasks_tx.send(LoomTask::FetchAndAddPools(pool_to_fetch)) {
        error!("Failed to send task: {}", e);
    }
    Ok(())
}
