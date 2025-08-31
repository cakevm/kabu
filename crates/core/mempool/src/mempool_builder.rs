use crate::MempoolComponent;
use eyre::Result;
use kabu_core_blockchain::AppState;
use kabu_core_components::{BuilderContext, PoolBuilder};
use reth_node_types::NodePrimitives;
use tokio::sync::broadcast;

/// Builder for the mempool component
#[derive(Clone, Default)]
pub struct MempoolBuilder;

impl MempoolBuilder {
    pub fn new() -> Self {
        Self
    }
}

impl<NP> PoolBuilder<AppState<NP>> for MempoolBuilder
where
    NP: NodePrimitives + Default + 'static,
{
    type Pool = MempoolComponent<NP>;

    async fn build_pool(self, ctx: &BuilderContext<AppState<NP>>) -> Result<Self::Pool> {
        let state = &ctx.state;

        // For now, create dummy channels - in real implementation these would come from EventChannels
        let (_mempool_tx, mempool_rx) = broadcast::channel(1000);
        let (_block_header_tx, block_header_rx) = broadcast::channel(1000);
        let (_block_tx, block_rx) = broadcast::channel(1000);
        let (mempool_events_tx, _) = broadcast::channel(1000);

        Ok(MempoolComponent::new(
            state.chain_parameters.clone(),
            state.mempool.clone(),
            mempool_rx,
            block_header_rx,
            block_rx,
            mempool_events_tx,
            None, // influxdb_tx
        ))
    }
}
