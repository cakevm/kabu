use super::{PendingTxStateChangeProcessorComponent, StateChangeArbSearcherComponent};
use crate::BackrunConfig;
use crate::block_state_change_processor::BlockStateChangeProcessorComponent;
use alloy_network::Network;
use alloy_provider::Provider;
use eyre::Result;
use influxdb::WriteQuery;
use kabu_core_components::Component;
use kabu_types_blockchain::ChainParameters;
use reth_tasks::TaskExecutor;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use kabu_evm_db::KabuDBError;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_blockchain::Mempool;
use kabu_types_entities::BlockHistory;
use kabu_types_events::{MarketEvents, MempoolEvents, MessageHealthEvent, MessageSwapCompose, StateUpdateEvent};
use kabu_types_market::{Market, MarketState};
use reth_node_types::NodePrimitives;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::marker::PhantomData;
use tracing::info;

pub struct StateChangeArbComponent<P, N, DB: Clone + Send + Sync + 'static, LDT: NodePrimitives + 'static> {
    backrun_config: BackrunConfig,
    client: P,

    market: Option<Arc<RwLock<Market>>>,

    mempool: Option<Arc<RwLock<Mempool<LDT>>>>,

    chain_parameters: ChainParameters,

    market_state: Option<Arc<RwLock<MarketState<DB>>>>,

    block_history: Option<Arc<RwLock<BlockHistory<DB, LDT>>>>,

    mempool_events_tx: Option<broadcast::Sender<MempoolEvents>>,

    market_events_tx: Option<broadcast::Sender<MarketEvents>>,

    swap_compose_channel_tx: Option<broadcast::Sender<MessageSwapCompose<DB, LDT>>>,

    pool_health_monitor_tx: Option<broadcast::Sender<MessageHealthEvent>>,

    influxdb_tx: Option<broadcast::Sender<WriteQuery>>,

    _n: PhantomData<N>,
}

impl<P, N, DB, LDT> StateChangeArbComponent<P, N, DB, LDT>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: NodePrimitives + 'static,
{
    pub fn new(client: P, backrun_config: BackrunConfig) -> StateChangeArbComponent<P, N, DB, LDT> {
        StateChangeArbComponent {
            backrun_config,
            client,
            chain_parameters: ChainParameters::ethereum(),
            market: None,
            mempool: None,
            block_history: None,
            market_state: None,
            mempool_events_tx: None,
            market_events_tx: None,
            swap_compose_channel_tx: None,
            pool_health_monitor_tx: None,
            influxdb_tx: None,
            _n: PhantomData,
        }
    }

    pub fn with_market(self, market: Arc<RwLock<Market>>) -> Self {
        Self { market: Some(market), ..self }
    }

    pub fn with_mempool(self, mempool: Arc<RwLock<Mempool<LDT>>>) -> Self {
        Self { mempool: Some(mempool), ..self }
    }

    pub fn with_market_state(self, market_state: Arc<RwLock<MarketState<DB>>>) -> Self {
        Self { market_state: Some(market_state), ..self }
    }

    pub fn with_block_history(self, block_history: Arc<RwLock<BlockHistory<DB, LDT>>>) -> Self {
        Self { block_history: Some(block_history), ..self }
    }

    pub fn with_mempool_events_channel(self, mempool_events_tx: broadcast::Sender<MempoolEvents>) -> Self {
        Self { mempool_events_tx: Some(mempool_events_tx), ..self }
    }

    pub fn with_market_events_channel(self, market_events_tx: broadcast::Sender<MarketEvents>) -> Self {
        Self { market_events_tx: Some(market_events_tx), ..self }
    }

    pub fn with_swap_compose_channel(self, swap_compose_channel_tx: broadcast::Sender<MessageSwapCompose<DB, LDT>>) -> Self {
        Self { swap_compose_channel_tx: Some(swap_compose_channel_tx), ..self }
    }

    pub fn with_pool_health_monitor_channel(self, pool_health_monitor_tx: broadcast::Sender<MessageHealthEvent>) -> Self {
        Self { pool_health_monitor_tx: Some(pool_health_monitor_tx), ..self }
    }

    pub fn with_influxdb_channel(self, influxdb_tx: broadcast::Sender<WriteQuery>) -> Self {
        Self { influxdb_tx: Some(influxdb_tx), ..self }
    }
}

impl<P, N, DB, LDT> Component for StateChangeArbComponent<P, N, DB, LDT>
where
    N: Network<TransactionRequest = alloy_rpc_types::TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    LDT: NodePrimitives + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        // Create state update channel for searcher
        let (searcher_pool_update_channel, _): (broadcast::Sender<StateUpdateEvent<DB, LDT>>, _) = broadcast::channel(100);

        // Spawn state change searcher
        let searcher = StateChangeArbSearcherComponent::new(self.backrun_config.clone())
            .with_channels(
                searcher_pool_update_channel.clone(),
                self.swap_compose_channel_tx.clone().unwrap(),
                self.pool_health_monitor_tx.clone().unwrap(),
                self.influxdb_tx,
            )
            .with_market(self.market.clone().unwrap());
        searcher.spawn(executor.clone())?;
        info!("State change searcher component started successfully");

        if self.mempool_events_tx.is_some() {
            let pending_tx_processor = PendingTxStateChangeProcessorComponent::new(self.client.clone())
                .with_channels(
                    self.market_events_tx.clone().unwrap(),
                    self.mempool_events_tx.clone().unwrap(),
                    searcher_pool_update_channel.clone(),
                )
                .with_market(self.market.clone().unwrap())
                .with_mempool(self.mempool.clone())
                .with_market_state(self.market_state.clone().unwrap());
            pending_tx_processor.spawn(executor.clone())?;
            info!("Pending tx state processor component started successfully");
        }

        if self.market_events_tx.is_some() {
            let block_processor = BlockStateChangeProcessorComponent::new().with_channels(
                self.chain_parameters.clone(),
                self.market.clone().unwrap(),
                self.block_history.clone().unwrap(),
                self.market_events_tx.clone().unwrap(),
                searcher_pool_update_channel.clone(),
            );
            block_processor.spawn(executor.clone())?;
            info!("Block state change processor component started successfully");
        }

        Ok(())
    }
    fn name(&self) -> &'static str {
        "StateChangeArbComponent"
    }
}
