use alloy_evm::EvmEnv;
use alloy_primitives::U256;
#[cfg(not(debug_assertions))]
use chrono::TimeDelta;
use eyre::{eyre, Result};
use influxdb::{Timestamp, WriteQuery};
use kabu_core_components::Component;
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
use reth_tasks::TaskExecutor;
use revm::context::{BlockEnv, CfgEnv};
use revm::context_interface::block::BlobExcessGasAndPrice;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, RwLock};
#[cfg(not(debug_assertions))]
use tracing::warn;
use tracing::{debug, error, info};

use crate::{BackrunConfig, SwapCalculator};

use kabu_core_blockchain::{Blockchain, Strategy};
use kabu_evm_db::{DatabaseHelpers, KabuDBError};
use kabu_types_blockchain::KabuDataTypes;
use kabu_types_entities::strategy_config::StrategyConfig;
use kabu_types_events::{
    BestTxSwapCompose, HealthEvent, Message, MessageHealthEvent, MessageSwapCompose, StateUpdateEvent, SwapComposeData, SwapComposeMessage,
    TxComposeData,
};
use kabu_types_market::{Market, PoolWrapper, SwapDirection, SwapPath};
use kabu_types_swap::{Swap, SwapError, SwapLine};

async fn state_change_arb_searcher_task<
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    LDT: KabuDataTypes,
>(
    thread_pool: Arc<ThreadPool>,
    backrun_config: BackrunConfig,
    state_update_event: StateUpdateEvent<DB, LDT>,
    market: Arc<RwLock<Market>>,
    swap_request_tx: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
    pool_health_monitor_tx: broadcast::Sender<MessageHealthEvent>,
    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
) -> Result<()> {
    debug!("Message received {} stuffing : {:?}", state_update_event.origin, state_update_event.stuffing_tx_hash());

    let mut db = state_update_event.market_state().clone();
    DatabaseHelpers::apply_geth_state_update_vec(&mut db, state_update_event.state_update().clone());

    let start_time_utc = chrono::Utc::now();

    let start_time = std::time::Instant::now();
    #[allow(clippy::mutable_key_type)]
    let mut swap_path_set: HashSet<SwapPath> = HashSet::new();

    let market_guard_read = market.read().await;
    debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.read acquired");

    for (pool, v) in state_update_event.directions().iter() {
        let pool_paths: Vec<SwapPath> = match market_guard_read.get_pool_paths(&pool.get_pool_id()) {
            Some(paths) => {
                // let pool_paths = pool_paths
                //     .into_iter()
                //     .enumerate()
                //     .filter(|(idx, path)| *idx < 100 || path.score.unwrap_or_default() > 0.9)
                //     .map(|(idx, path)| path)
                //     .collect::<Vec<_>>();
                // pool_paths
                paths
                    .into_iter()
                    .enumerate()
                    .filter(|(idx, swap_path)| {
                        *idx < 100 || swap_path.score.unwrap_or_default() > 0.97
                        //&& !swap_path.pools.iter().any(|pool| market_guard_read.is_pool_disabled(&pool.get_pool_id()))
                    })
                    .map(|(_, swap_path)| swap_path)
                    .collect::<Vec<_>>()
            }

            None => {
                let mut pool_direction: BTreeMap<PoolWrapper, Vec<SwapDirection>> = BTreeMap::new();
                pool_direction.insert(pool.clone(), v.clone());
                market_guard_read.build_swap_path_vec(&pool_direction).unwrap_or_default()
            }
        };

        for pool_path in pool_paths {
            swap_path_set.insert(pool_path);
        }
    }
    drop(market_guard_read);
    debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.read released");

    let swap_path_vec: Vec<SwapPath> = swap_path_set.into_iter().collect();

    if swap_path_vec.is_empty() {
        debug!(
            request=?state_update_event.stuffing_txs_hashes().first(),
            elapsed=start_time.elapsed().as_micros(),
            "No swap path built",

        );
        return Err(eyre!("NO_SWAP_PATHS"));
    }

    // Log details about the swap paths and affected pools
    debug!("Affected pools: {} unique pools, generated {} swap paths", state_update_event.directions().len(), swap_path_vec.len());

    // If we have very few pools but many paths, log a warning
    if state_update_event.directions().len() <= 3 && swap_path_vec.len() > 50 {
        debug!(
            "Warning: {} swap paths generated from only {} pools - this may indicate duplicate path generation",
            swap_path_vec.len(),
            state_update_event.directions().len()
        );
    }

    info!("Calculation started: swap_path_vec_len={} elapsed={}", swap_path_vec.len(), start_time.elapsed().as_micros());

    let channel_len = swap_path_vec.len();
    let (swap_path_tx, mut swap_line_rx) = tokio::sync::mpsc::channel(channel_len);

    let block_env = BlockEnv {
        number: U256::from(state_update_event.next_block_number),
        timestamp: U256::from(state_update_event.next_block_timestamp),
        basefee: state_update_event.next_base_fee,
        blob_excess_gas_and_price: Some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 0 }),
        ..Default::default()
    };
    let evm_env = EvmEnv::new(CfgEnv::new(), block_env);
    let market_state_clone = db.clone();
    let swap_path_vec_len = swap_path_vec.len();

    let _tasks = tokio::task::spawn(async move {
        thread_pool.install(|| {
            swap_path_vec.into_par_iter().for_each_with((&swap_path_tx, &market_state_clone, evm_env), |(_req, _db, evm_env), item| {
                let mut mut_item: SwapLine = SwapLine { path: item, ..Default::default() };

                let calc_result = SwapCalculator::calculate(&mut mut_item, &market_state_clone, evm_env.clone());

                match calc_result {
                    Ok(_) => {
                        debug!("Calc result received: {}", mut_item);

                        if let Ok(profit) = mut_item.profit() {
                            if profit.is_positive() && mut_item.abs_profit_eth() > U256::from(state_update_event.next_base_fee * 100_000) {
                                if let Err(error) = swap_path_tx.try_send(Ok(mut_item)) {
                                    error!(%error, "swap_path_tx.try_send")
                                }
                            } else {
                                debug!("profit is not enough")
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Swap error: {:?}", e);

                        if let Err(error) = swap_path_tx.try_send(Err(e)) {
                            error!(%error, "try_send to swap_path_tx")
                        }
                    }
                }
            });
        });
        debug!(elapsed = start_time.elapsed().as_micros(), "Calculation iteration finished");
    });

    debug!(elapsed = start_time.elapsed().as_micros(), "Calculation results receiver started");

    let swap_request_tx_clone = swap_request_tx.clone();
    let pool_health_monitor_tx_clone = pool_health_monitor_tx.clone();

    let mut answers = 0;

    let mut best_answers = BestTxSwapCompose::new_with_pct(U256::from(5000)); // Only send swaps that are at least 50% as good as the best

    let mut failed_pools: HashSet<SwapError> = HashSet::new();

    while let Some(swap_line_result) = swap_line_rx.recv().await {
        match swap_line_result {
            Ok(swap_line) => {
                // Add minimum profit filter: skip if profit is less than 0.001 ETH
                let min_profit_wei = U256::from(1_000_000_000_000_000u64); // 0.001 ETH
                if swap_line.abs_profit_eth() < min_profit_wei {
                    continue;
                }

                let prepare_request = SwapComposeMessage::Prepare(SwapComposeData {
                    tx_compose: TxComposeData::<LDT> {
                        eoa: backrun_config.eoa(),
                        next_block_number: state_update_event.next_block_number,
                        next_block_timestamp: state_update_event.next_block_timestamp,
                        next_block_base_fee: state_update_event.next_base_fee,
                        gas: swap_line.gas_used.unwrap_or(300000),
                        stuffing_txs: state_update_event.stuffing_txs().clone(),
                        stuffing_txs_hashes: state_update_event.stuffing_txs_hashes.clone(),
                        ..TxComposeData::default()
                    },
                    swap: Swap::BackrunSwapLine(swap_line),
                    origin: Some(state_update_event.origin.clone()),
                    tips_pct: Some(state_update_event.tips_pct),
                    poststate: Some(db.clone()),
                    poststate_update: Some(state_update_event.state_update().clone()),
                    ..SwapComposeData::default()
                });

                if !backrun_config.smart() || best_answers.check(&prepare_request) {
                    if let Err(e) = swap_request_tx_clone.send(Message::new(prepare_request)) {
                        error!("swap_request_tx_clone.send {}", e)
                    }
                }
            }
            Err(swap_error) => {
                if failed_pools.insert(swap_error.clone()) {
                    if let Err(e) = pool_health_monitor_tx_clone.send(Message::new(HealthEvent::PoolSwapError(swap_error))) {
                        error!("try_send to pool_health_monitor error : {:?}", e)
                    }
                }
            }
        }

        answers += 1;
    }

    let stuffing_tx_hash = state_update_event.stuffing_tx_hash();
    let elapsed = start_time.elapsed().as_micros();
    info!(
        origin = %state_update_event.origin,
        swap_path_vec_len,
        answers,
        elapsed,
        stuffing_hash = %stuffing_tx_hash,
        "Calculation finished"
    );

    let write_query = WriteQuery::new(Timestamp::from(start_time_utc), "calculations")
        .add_field("calculations", swap_path_vec_len as u64)
        .add_field("answers", answers as u64)
        .add_field("elapsed", elapsed as u64)
        .add_tag("origin", state_update_event.origin)
        .add_tag("stuffing", stuffing_tx_hash.to_string());

    if let Some(tx) = influxdb_write_channel_tx {
        if let Err(e) = tx.send(write_query) {
            error!("Failed to send block latency to influxdb: {:?}", e);
        }
    }

    Ok(())
}

pub async fn state_change_arb_searcher_worker<
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    LDT: KabuDataTypes + 'static,
>(
    backrun_config: BackrunConfig,
    market: Arc<RwLock<Market>>,
    search_request_rx: broadcast::Receiver<StateUpdateEvent<DB, LDT>>,
    swap_request_tx: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
    pool_health_monitor_tx: broadcast::Sender<MessageHealthEvent>,
    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
) -> Result<()> {
    let mut search_request_receiver = search_request_rx.resubscribe();

    let cpus = num_cpus::get();
    let tasks = (cpus * 5) / 10;
    info!("Starting state arb searcher cpus={cpus}, tasks={tasks}");
    let thread_pool = Arc::new(ThreadPoolBuilder::new().num_threads(tasks).build()?);

    // Track recent state updates to prevent duplicate calculations for the same pools
    let mut recent_pool_updates: HashSet<(u64, Vec<PoolWrapper>)> = HashSet::new();
    let mut last_block_number = 0u64;

    loop {
        tokio::select! {
                msg = search_request_receiver.recv() => {
                let pool_update_msg : Result<StateUpdateEvent<DB, LDT>, RecvError> = msg;
                if let Ok(msg) = pool_update_msg {
                    // Clear recent updates on new block
                    if msg.next_block_number != last_block_number {
                        recent_pool_updates.clear();
                        last_block_number = msg.next_block_number;
                    }

                    // Create a sorted list of affected pools for consistent hashing
                    let mut affected_pools: Vec<PoolWrapper> = msg.directions().keys().cloned().collect();
                    affected_pools.sort_by_key(|p| p.get_pool_id());

                    let pool_update_key = (msg.next_block_number, affected_pools);

                    // Skip if we've already processed these exact pools in this block
                    if recent_pool_updates.contains(&pool_update_key) {
                        debug!(
                            "Skipping duplicate state update for block {} with {} pools",
                            msg.next_block_number,
                            pool_update_key.1.len()
                        );
                        continue;
                    }

                    recent_pool_updates.insert(pool_update_key);

                    tokio::task::spawn(
                        state_change_arb_searcher_task(
                            thread_pool.clone(),
                            backrun_config.clone(),
                            msg,
                            market.clone(),
                            swap_request_tx.clone(),
                            pool_health_monitor_tx.clone(),
                            influxdb_write_channel_tx.clone(),
                        )
                    );
                }
            }
        }
    }
}

pub struct StateChangeArbSearcherComponent<DB: Clone + Send + Sync + 'static, LDT: KabuDataTypes + 'static> {
    backrun_config: BackrunConfig,

    market: Option<Arc<RwLock<Market>>>,

    state_update_rx: Option<broadcast::Sender<StateUpdateEvent<DB, LDT>>>,

    swap_tx: Option<broadcast::Sender<MessageSwapCompose<DB, LDT>>>,

    pool_health_monitor_tx: Option<broadcast::Sender<MessageHealthEvent>>,

    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
}

impl<
        DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
        LDT: KabuDataTypes + 'static,
    > StateChangeArbSearcherComponent<DB, LDT>
{
    pub fn new(backrun_config: BackrunConfig) -> StateChangeArbSearcherComponent<DB, LDT> {
        StateChangeArbSearcherComponent {
            backrun_config,
            market: None,
            state_update_rx: None,
            swap_tx: None,
            pool_health_monitor_tx: None,
            influxdb_write_channel_tx: None,
        }
    }

    pub fn on_bc(self, bc: &Blockchain<LDT>, strategy: &Strategy<DB, LDT>) -> Self {
        Self {
            market: Some(bc.market()),
            pool_health_monitor_tx: Some(bc.health_monitor_channel()),
            swap_tx: Some(strategy.swap_compose_channel()),
            state_update_rx: Some(strategy.state_update_channel()),
            influxdb_write_channel_tx: bc.influxdb_write_channel(),
            ..self
        }
    }

    pub fn with_channels(
        self,
        state_update_rx: broadcast::Sender<StateUpdateEvent<DB, LDT>>,
        swap_tx: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
        pool_health_monitor_tx: broadcast::Sender<MessageHealthEvent>,
        influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
    ) -> Self {
        Self {
            state_update_rx: Some(state_update_rx),
            swap_tx: Some(swap_tx),
            pool_health_monitor_tx: Some(pool_health_monitor_tx),
            influxdb_write_channel_tx,
            ..self
        }
    }

    pub fn with_market(self, market: Arc<RwLock<Market>>) -> Self {
        Self { market: Some(market), ..self }
    }
}

impl<DB, LDT> Component for StateChangeArbSearcherComponent<DB, LDT>
where
    DB: Database<Error = KabuDBError> + DatabaseRef<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    LDT: KabuDataTypes + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let state_update_rx = self.state_update_rx.ok_or_else(|| eyre!("state_update_rx not set"))?.subscribe();
        let swap_tx = self.swap_tx.ok_or_else(|| eyre!("swap_tx not set"))?;
        let market = self.market.ok_or_else(|| eyre!("market not set"))?;
        let pool_health_monitor_tx = self.pool_health_monitor_tx.ok_or_else(|| eyre!("pool_health_monitor_tx not set"))?;
        let influxdb_write_channel = self.influxdb_write_channel_tx;
        let backrun_config = self.backrun_config.clone();

        executor.spawn_critical(name, async move {
            if let Err(e) = state_change_arb_searcher_worker(
                backrun_config,
                market,
                state_update_rx,
                swap_tx,
                pool_health_monitor_tx,
                influxdb_write_channel,
            )
            .await
            {
                error!("state_change_arb_searcher_worker failed: {}", e);
            }
        });

        Ok(())
    }

    fn name(&self) -> &'static str {
        "StateChangeArbSearcherComponent"
    }
}
