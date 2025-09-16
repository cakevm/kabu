use super::affected_pools_state::get_affected_pools_from_state_update;
use alloy_consensus::BlockHeader;
use eyre::{Result, eyre};
use kabu_core_components::Component;
use kabu_types_blockchain::ChainParameters;
use kabu_types_entities::BlockHistory;
use kabu_types_events::{MarketEvents, StateUpdateEvent};
use kabu_types_market::Market;
use reth_node_types::NodePrimitives;
use reth_tasks::TaskExecutor;
use revm::DatabaseRef;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{RwLock, broadcast};
use tracing::error;

pub async fn block_state_change_worker<DB, LDT>(
    chain_parameters: ChainParameters,
    market: Arc<RwLock<Market>>,
    block_history: Arc<RwLock<BlockHistory<DB, LDT>>>,
    mut market_events_rx: broadcast::Receiver<MarketEvents>,
    state_updates_broadcaster: broadcast::Sender<StateUpdateEvent<DB, LDT>>,
) where
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: NodePrimitives,
    LDT::BlockHeader: BlockHeader,
{
    loop {
        let market_event = match market_events_rx.recv().await {
            Ok(market_event) => market_event,
            Err(e) => match e {
                RecvError::Closed => {
                    error!("Market events txs channel closed");
                    error!("MARKET_EVENTS_RX_CLOSED");
                    break;
                }
                RecvError::Lagged(lag) => {
                    error!("Market events txs channel lagged by {} messages", lag);
                    continue;
                }
            },
        };
        let block_hash = match market_event {
            MarketEvents::BlockStateUpdate { block_hash } => block_hash,
            _ => continue,
        };

        let Some(block_history_entry) = block_history.read().await.get_block_history_entry(&block_hash).cloned() else {
            error!("Block history entry not found in block history: {:?}", block_hash);
            continue;
        };

        let Some(block_state_entry) = block_history.read().await.get_block_state(&block_hash).cloned() else {
            error!("Block state not found in block history: {:?}", block_hash);
            continue;
        };

        let Some(state_update) = block_history_entry.state_update.clone() else {
            error!("Block {:?} has no state update", block_hash);
            continue;
        };

        let affected_pools = get_affected_pools_from_state_update(market.clone(), &state_update).await;

        if affected_pools.is_empty() {
            error!("Could not get affected pools for block {:?}", block_hash);
            continue;
        };

        let next_block_number = block_history_entry.header.number() + 1;
        let next_block_timestamp = block_history_entry.header.timestamp() + 12;
        let base_fee = block_history_entry.header.base_fee_per_gas().unwrap_or_default();
        let gas_used = block_history_entry.header.gas_used();
        let gas_limit = block_history_entry.header.gas_limit();
        let next_base_fee = chain_parameters.calc_next_block_base_fee(gas_used, gas_limit, base_fee);

        let request = StateUpdateEvent::new(
            next_block_number,
            next_block_timestamp,
            next_base_fee,
            block_state_entry,
            state_update,
            None,
            affected_pools,
            Vec::new(),
            Vec::new(),
            "block_searcher".to_string(),
            90_00,
        );
        match state_updates_broadcaster.send(request) {
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "ERROR_RUNNING_SYNC"),
        }
    }
}

pub struct BlockStateChangeProcessorComponent<DB: Clone + Send + Sync + 'static, LDT: NodePrimitives + 'static> {
    chain_parameters: ChainParameters,
    market: Option<Arc<RwLock<Market>>>,
    block_history: Option<Arc<RwLock<BlockHistory<DB, LDT>>>>,
    market_events_rx: Option<broadcast::Sender<MarketEvents>>,
    state_updates_tx: Option<broadcast::Sender<StateUpdateEvent<DB, LDT>>>,
}

impl<DB: DatabaseRef + Send + Sync + Clone + 'static, LDT: NodePrimitives> BlockStateChangeProcessorComponent<DB, LDT> {
    pub fn new() -> BlockStateChangeProcessorComponent<DB, LDT> {
        BlockStateChangeProcessorComponent {
            chain_parameters: ChainParameters::ethereum(),
            market: None,
            block_history: None,
            market_events_rx: None,
            state_updates_tx: None,
        }
    }

    pub fn with_channels(
        self,
        chain_parameters: ChainParameters,
        market: Arc<RwLock<Market>>,
        block_history: Arc<RwLock<BlockHistory<DB, LDT>>>,
        market_events_rx: broadcast::Sender<MarketEvents>,
        state_updates_tx: broadcast::Sender<StateUpdateEvent<DB, LDT>>,
    ) -> Self {
        Self {
            chain_parameters,
            market: Some(market),
            block_history: Some(block_history),
            market_events_rx: Some(market_events_rx),
            state_updates_tx: Some(state_updates_tx),
        }
    }
}

impl<DB: DatabaseRef + Send + Sync + Clone + 'static, LDT: NodePrimitives> Default for BlockStateChangeProcessorComponent<DB, LDT> {
    fn default() -> Self {
        Self::new()
    }
}

impl<DB: DatabaseRef + Send + Sync + Clone + 'static, LDT: NodePrimitives> Component for BlockStateChangeProcessorComponent<DB, LDT> {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let market_events_rx = self.market_events_rx.ok_or_else(|| eyre!("market_events_rx not set"))?.subscribe();
        let state_updates_tx = self.state_updates_tx.ok_or_else(|| eyre!("state_updates_tx not set"))?;
        let market = self.market.ok_or_else(|| eyre!("market not set"))?;
        let block_history = self.block_history.ok_or_else(|| eyre!("block_history not set"))?;

        executor.spawn_critical(
            name,
            block_state_change_worker(self.chain_parameters, market, block_history, market_events_rx, state_updates_tx),
        );

        Ok(())
    }

    fn name(&self) -> &'static str {
        "BlockStateChangeProcessorComponent"
    }
}
