use kabu_evm_db::DatabaseKabuExt;
use kabu_types_entities::BlockHistoryState;
use kabu_types_events::{MessageSwapCompose, StateUpdateEvent};
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use revm::{Database, DatabaseCommit, DatabaseRef};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct Strategy<DB: Clone + Send + Sync + 'static, LDT: NodePrimitives + 'static = EthPrimitives> {
    swap_compose_channel: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
    state_update_channel: broadcast::Sender<StateUpdateEvent<DB, LDT>>,
}

impl<
        DB: DatabaseRef + Database + DatabaseCommit + BlockHistoryState<LDT> + DatabaseKabuExt + Send + Sync + Clone + Default + 'static,
        LDT: NodePrimitives,
    > Default for Strategy<DB, LDT>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
        DB: DatabaseRef + Database + DatabaseCommit + BlockHistoryState<LDT> + DatabaseKabuExt + Send + Sync + Clone + Default + 'static,
        LDT: NodePrimitives,
    > Strategy<DB, LDT>
{
    pub fn new() -> Self {
        let compose_channel: broadcast::Sender<MessageSwapCompose<DB, LDT>> = broadcast::channel(100).0;
        let state_update_channel: broadcast::Sender<StateUpdateEvent<DB, LDT>> = broadcast::channel(100).0;
        Strategy { swap_compose_channel: compose_channel, state_update_channel }
    }
}

impl<DB: Send + Sync + Clone + 'static, LDT: NodePrimitives> Strategy<DB, LDT> {
    pub fn swap_compose_channel(&self) -> broadcast::Sender<MessageSwapCompose<DB, LDT>> {
        self.swap_compose_channel.clone()
    }

    pub fn state_update_channel(&self) -> broadcast::Sender<StateUpdateEvent<DB, LDT>> {
        self.state_update_channel.clone()
    }
}
