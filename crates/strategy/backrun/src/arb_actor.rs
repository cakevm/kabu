use super::{PendingTxStateChangeProcessorActor, StateChangeArbSearcherActor};
use crate::block_state_change_processor::BlockStateChangeProcessorActor;
use crate::BackrunConfig;
use alloy_network::Network;
use alloy_provider::Provider;
use influxdb::WriteQuery;
use kabu_core_actors::{Accessor, Actor, ActorResult, Broadcaster, Consumer, Producer, SharedState, WorkerResult};
use kabu_core_actors_macros::{Accessor, Consumer, Producer};
use kabu_evm_db::KabuDBError;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_blockchain::{KabuDataTypesEVM, Mempool};
use kabu_types_entities::{BlockHistory, LatestBlock, Market, MarketState};
use kabu_types_events::{MarketEvents, MempoolEvents, MessageHealthEvent, MessageSwapCompose};
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::marker::PhantomData;
use tokio::task::JoinHandle;
use tracing::info;

#[derive(Accessor, Consumer, Producer)]
pub struct StateChangeArbActor<P, N, DB: Clone + Send + Sync + 'static, LDT: KabuDataTypesEVM + 'static> {
    backrun_config: BackrunConfig,
    client: P,
    use_blocks: bool,
    use_mempool: bool,
    #[accessor]
    market: Option<SharedState<Market>>,
    #[accessor]
    mempool: Option<SharedState<Mempool<LDT>>>,
    #[accessor]
    latest_block: Option<SharedState<LatestBlock<LDT>>>,
    #[accessor]
    market_state: Option<SharedState<MarketState<DB>>>,
    #[accessor]
    block_history: Option<SharedState<BlockHistory<DB, LDT>>>,
    #[consumer]
    mempool_events_tx: Option<Broadcaster<MempoolEvents>>,
    #[consumer]
    market_events_tx: Option<Broadcaster<MarketEvents>>,
    #[producer]
    compose_channel_tx: Option<Broadcaster<MessageSwapCompose<DB, LDT>>>,
    #[producer]
    pool_health_monitor_tx: Option<Broadcaster<MessageHealthEvent>>,
    #[producer]
    influxdb_write_channel_tx: Option<Broadcaster<WriteQuery>>,

    _n: PhantomData<N>,
}

impl<P, N, DB, LDT> StateChangeArbActor<P, N, DB, LDT>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: KabuDataTypesEVM + 'static,
{
    pub fn new(client: P, use_blocks: bool, use_mempool: bool, backrun_config: BackrunConfig) -> StateChangeArbActor<P, N, DB, LDT> {
        StateChangeArbActor {
            backrun_config,
            client,
            use_blocks,
            use_mempool,
            market: None,
            mempool: None,
            latest_block: None,
            block_history: None,
            market_state: None,
            mempool_events_tx: None,
            market_events_tx: None,
            compose_channel_tx: None,
            pool_health_monitor_tx: None,
            influxdb_write_channel_tx: None,
            _n: PhantomData,
        }
    }
}

impl<P, N, DB, LDT> Actor for StateChangeArbActor<P, N, DB, LDT>
where
    N: Network<TransactionRequest = LDT::TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    LDT: KabuDataTypesEVM + 'static,
{
    fn start(&self) -> ActorResult {
        let searcher_pool_update_channel = Broadcaster::new(100);
        let mut tasks: Vec<JoinHandle<WorkerResult>> = Vec::new();

        let mut state_update_searcher = StateChangeArbSearcherActor::new(self.backrun_config.clone());
        let mut searcher_builder = state_update_searcher
            .access(self.market.clone().unwrap())
            .consume(searcher_pool_update_channel.clone())
            .produce(self.compose_channel_tx.clone().unwrap())
            .produce(self.pool_health_monitor_tx.clone().unwrap());

        if let Some(influx_channel) = self.influxdb_write_channel_tx.clone() {
            searcher_builder = searcher_builder.produce(influx_channel);
        }

        match searcher_builder.start() {
            Err(e) => {
                panic!("{}", e)
            }
            Ok(r) => {
                tasks.extend(r);
                info!("State change searcher actor started successfully")
            }
        }

        if self.mempool_events_tx.is_some() && self.use_mempool {
            let mut pending_tx_state_processor = PendingTxStateChangeProcessorActor::new(self.client.clone());
            match pending_tx_state_processor
                .access(self.mempool.clone().unwrap())
                .access(self.latest_block.clone().unwrap())
                .access(self.market.clone().unwrap())
                .access(self.market_state.clone().unwrap())
                .consume(self.mempool_events_tx.clone().unwrap())
                .consume(self.market_events_tx.clone().unwrap())
                .produce(searcher_pool_update_channel.clone())
                .start()
            {
                Err(e) => {
                    panic!("{}", e)
                }
                Ok(r) => {
                    tasks.extend(r);
                    info!("Pending tx state actor started successfully")
                }
            }
        }

        if self.market_events_tx.is_some() && self.use_blocks {
            let mut block_state_processor = BlockStateChangeProcessorActor::new();
            match block_state_processor
                .access(self.market.clone().unwrap())
                .access(self.block_history.clone().unwrap())
                .consume(self.market_events_tx.clone().unwrap())
                .produce(searcher_pool_update_channel.clone())
                .start()
            {
                Err(e) => {
                    panic!("{}", e)
                }
                Ok(r) => {
                    tasks.extend(r);
                    info!("Block change state actor started successfully")
                }
            }
        }

        Ok(tasks)
    }

    fn name(&self) -> &'static str {
        "StateChangeArbActor"
    }
}
