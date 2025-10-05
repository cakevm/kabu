use alloy_consensus::Transaction as _;
use alloy_consensus::transaction::TxHashRef;
use alloy_eips::BlockNumberOrTag;
use alloy_evm::EvmEnv;
use alloy_network::Network;
use alloy_primitives::{Address, BlockNumber, TxHash, U256};
use alloy_provider::Provider;
use alloy_rpc_types::state::StateOverride;
use alloy_rpc_types::{BlockOverrides, TransactionInput, TransactionRequest};
use alloy_rpc_types_trace::geth::GethDebugTracingCallOptions;
use eyre::{Result, eyre};
use kabu_core_components::Component;
use kabu_evm_db::KabuDBError;
use lazy_static::lazy_static;
use revm::context::{BlockEnv, CfgEnv};
use revm::context_interface::block::BlobExcessGasAndPrice;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, warn};

use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_blockchain::{GethStateUpdateVec, Mempool, TRACING_CALL_OPTS, debug_trace_call_diff};
use kabu_types_events::{MarketEvents, MempoolEvents, StateUpdateEvent};
use kabu_types_market::{Market, MarketState};
use kabu_types_market::{accounts_vec_len, storage_vec_len};
use reth_node_types::NodePrimitives;
use reth_primitives_traits::SignerRecoverable;
use reth_tasks::TaskExecutor;

use super::affected_pools_code::{get_affected_pools_from_code, is_pool_code};
use super::affected_pools_state::get_affected_pools_from_state_update;

lazy_static! {
    static ref COINBASE: Address = "0x1f9090aaE28b8a3dCeaDf281B0F12828e676c326".parse().unwrap();
}

/// Process a pending tx from the mempool
#[allow(clippy::too_many_arguments)]
pub async fn pending_tx_state_change_task<P, N, DB, LDT>(
    client: P,
    tx_hash: TxHash,
    market: Arc<RwLock<Market>>,
    mempool: Arc<RwLock<Mempool<LDT>>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    affecting_tx: Arc<RwLock<HashMap<TxHash, bool>>>,
    cur_block_number: BlockNumber,
    cur_block_time: u64,
    cur_next_base_fee: u64,
    cur_state_override: StateOverride,
    state_updates_broadcaster: broadcast::Sender<StateUpdateEvent<DB, LDT>>,
) -> Result<()>
where
    N: Network<TransactionRequest = alloy_rpc_types::TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Database + DatabaseCommit + Clone + Send + Sync + 'static,
    LDT: NodePrimitives + 'static,
{
    let mut state_update_vec: GethStateUpdateVec = Vec::new();
    let mut state_required_vec: GethStateUpdateVec = Vec::new();

    let mut merged_state_update_vec: GethStateUpdateVec = Vec::new();

    let mempool_tx = match mempool.read().await.get_tx_by_hash(&tx_hash).cloned() {
        Some(tx) => tx,
        None => return Err(eyre!("MEMPOOL_TX_NOT_FOUND")),
    };

    let tx = match mempool_tx.tx.clone() {
        Some(tx) => tx,
        None => return Err(eyre!("NO_TX_IN_MEMPOOL")),
    };

    let source = mempool_tx.source.clone();

    // Convert transaction to TransactionRequest
    // Extract the signer using recover_signer() for the from address
    let from = tx.recover_signer().map_err(|e| eyre!("Failed to recover signer: {}", e))?;
    let transaction_request = TransactionRequest::default()
        .from(from)
        .to(tx.to().unwrap_or_default())
        .value(tx.value())
        .input(TransactionInput::new(tx.input().clone()))
        .nonce(tx.nonce())
        .gas_limit(tx.gas_limit())
        .max_fee_per_gas(tx.max_fee_per_gas())
        .max_priority_fee_per_gas(tx.max_priority_fee_per_gas().unwrap_or_default());

    // let transaction_type = transaction_request.transaction_type.unwrap_or_default();
    // if transaction_type == LEGACY_TX_TYPE_ID || transaction_type == EIP2930_TX_TYPE_ID {
    //     match transaction_request.gas_price {
    //         Some(g) => {
    //             if g < cur_next_base_fee as u128 {
    //                 transaction_request.set_gas_price(cur_next_base_fee as u128);
    //             }
    //         }
    //         None => {
    //             error!(
    //                 "No gas price for gas_price={:?}, max_fee_per_gas={:?}, max_priority_fee_per_gas={:?}, hash={:?}",
    //                 transaction_request.gas_price,
    //                 transaction_request.max_fee_per_gas,
    //                 transaction_request.max_priority_fee_per_gas,
    //                 mempool_tx.tx_hash
    //             );
    //             return Err(eyre!("NO_GAS_PRICE"));
    //         }
    //     }
    // } else if transaction_type == EIP1559_TX_TYPE_ID {
    //     match transaction_request.max_fee_per_gas {
    //         Some(g) => {
    //             if g < cur_next_base_fee as u128 {
    //                 transaction_request.set_max_fee_per_gas(cur_next_base_fee as u128);
    //             }
    //         }
    //         None => {
    //             error!(
    //                 "No base fee for gas_price={:?}, max_fee_per_gas={:?}, max_priority_fee_per_gas={:?}, hash={:?}",
    //                 transaction_request.gas_price,
    //                 transaction_request.max_fee_per_gas,
    //                 transaction_request.max_priority_fee_per_gas,
    //                 mempool_tx.tx_hash
    //             );
    //             return Err(eyre!("NO_BASE_FEE"));
    //         }
    //     }
    // } else if transaction_type == EIP4844_TX_TYPE_ID {
    //     // ignore blob tx
    //     debug!("Ignore EIP4844 transaction: hash={:?}", mempool_tx.tx_hash);
    //     return Ok(());
    // } else {
    //     warn!("Unknown transaction type: type={}, hash={:?}", transaction_type, mempool_tx.tx_hash);
    //     return Err(eyre!("UNKNOWN_TX_TYPE"));
    // }

    let call_opts: GethDebugTracingCallOptions = GethDebugTracingCallOptions {
        block_overrides: Some(BlockOverrides {
            number: Some(U256::from(cur_block_number)),
            time: Some(cur_block_time),
            coinbase: Some(*COINBASE),
            base_fee: Some(U256::from(cur_next_base_fee)),
            ..Default::default()
        }),
        state_overrides: Some(cur_state_override.clone()),
        ..TRACING_CALL_OPTS.clone()
    };

    if !(*affecting_tx.read().await.get(&tx_hash).unwrap_or(&true)) {
        return Err(eyre!("NON_AFFECTING_TX"));
    }

    let diff_trace_result =
        debug_trace_call_diff(client.clone(), transaction_request, BlockNumberOrTag::Latest.into(), Some(call_opts)).await;
    match diff_trace_result {
        Ok((pre, post)) => {
            state_required_vec.push(pre.clone());
            state_update_vec.push(post.clone());

            merged_state_update_vec.push(pre);
            merged_state_update_vec.push(post);
        }
        Err(error) => {
            let tx_hash = *tx.tx_hash();
            mempool.write().await.set_failed(tx_hash);
            debug!(block=cur_block_number, %tx_hash, %error, "debug_trace_call error for");
        }
    }

    let affected_pools = get_affected_pools_from_state_update(market.clone(), &state_update_vec).await;

    let accounts_len = accounts_vec_len(&state_update_vec);
    let storage_len = storage_vec_len(&state_update_vec);

    debug!(%tx_hash, %source, pools = affected_pools.len(), accounts = accounts_len, storage = storage_len, "Mempool affected pools");

    affecting_tx.write().await.insert(tx_hash, !affected_pools.is_empty());

    // Use the passed in block number and time (already incremented in worker)
    let next_block_number = cur_block_number;
    let next_block_timestamp = cur_block_time;

    if !affected_pools.is_empty() {
        let cur_state_db = market_state.read().await.state_db.clone();
        let request = StateUpdateEvent::new(
            next_block_number,
            next_block_timestamp,
            cur_next_base_fee,
            cur_state_db,
            state_update_vec,
            Some(state_required_vec.clone()),
            affected_pools,
            vec![tx_hash],
            vec![mempool_tx.tx.clone().unwrap()],
            "pending_tx_searcher".to_string(),
            9000,
        );
        if let Err(e) = state_updates_broadcaster.send(request) {
            error!("state_updates_broadcaster : {}", e)
        }
    }

    if is_pool_code(&merged_state_update_vec) {
        let evm_env = EvmEnv {
            cfg_env: CfgEnv::default(),
            block_env: BlockEnv {
                number: U256::from(next_block_number),
                timestamp: U256::from(next_block_timestamp),
                basefee: cur_next_base_fee,
                blob_excess_gas_and_price: Some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 0 }),
                ..Default::default()
            },
        };
        match get_affected_pools_from_code(client, market.clone(), &merged_state_update_vec, &evm_env).await {
            Ok(affected_pools) => {
                match affecting_tx.write().await.entry(tx_hash) {
                    Entry::Occupied(mut v) => {
                        if !v.get() {
                            v.insert(!affected_pools.is_empty());
                        }
                    }
                    Entry::Vacant(v) => {
                        v.insert(!affected_pools.is_empty());
                    }
                };

                debug!("Mempool code pools {} {} update len : {}", tx_hash, source, affected_pools.len());

                {
                    // Use the passed in block number and time (already incremented in worker)
                    let block_number = cur_block_number;
                    let block_timestamp = cur_block_time;

                    if !affected_pools.is_empty() {
                        let cur_state_db = market_state.read().await.state_db.clone();

                        let request = StateUpdateEvent::new(
                            block_number,
                            block_timestamp,
                            cur_next_base_fee,
                            cur_state_db,
                            merged_state_update_vec,
                            None,
                            affected_pools,
                            vec![tx_hash],
                            vec![mempool_tx.tx.unwrap()],
                            "poolcode_searcher".to_string(),
                            3000,
                        );
                        if let Err(e) = state_updates_broadcaster.send(request) {
                            error!("state_updates_broadcaster : {}", e)
                        }
                    }
                }
            }
            Err(e) => {
                debug!("code affected pools error : {e}")
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn pending_tx_state_change_worker<P, N, DB, LDT>(
    client: P,
    market: Arc<RwLock<Market>>,
    mempool: Arc<RwLock<Mempool<LDT>>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    mempool_events_rx: broadcast::Sender<MempoolEvents>,
    market_events_rx: broadcast::Sender<MarketEvents>,
    state_updates_broadcaster: broadcast::Sender<StateUpdateEvent<DB, LDT>>,
) -> Result<()>
where
    N: Network<TransactionRequest = alloy_rpc_types::TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Database + DatabaseCommit + Clone + Send + Sync + 'static,
    LDT: NodePrimitives + 'static,
{
    let mut mempool_events_receiver = mempool_events_rx.subscribe();
    let mut market_events_receiver = market_events_rx.subscribe();

    let affecting_tx: Arc<RwLock<HashMap<TxHash, bool>>> = Arc::new(RwLock::new(HashMap::new()));
    let mut cur_next_base_fee = 0;
    let mut cur_block_number: Option<BlockNumber> = None;
    let mut cur_block_time: Option<u64> = None;
    let mut cur_state_override: StateOverride = StateOverride::default();

    loop {
        tokio::select! {
            msg = market_events_receiver.recv() => {
                if let Ok(msg) = msg {
                    let market_event_msg : MarketEvents = msg;
                    if let MarketEvents::BlockHeaderUpdate{ block_number, block_hash, timestamp, base_fee, next_base_fee } = market_event_msg {
                        debug!("Block header update {} {} base_fee {} ", block_number, block_hash, base_fee);
                        cur_block_number = Some( block_number + 1);
                        cur_block_time = Some(timestamp + 12 );
                        cur_next_base_fee = next_base_fee;

                        for _counter in 0..5  {
                            if let Ok(msg) = market_events_receiver.recv().await
                                && matches!(msg, MarketEvents::BlockStateUpdate{..} ) {
                                    // TODO: Get state override from elsewhere if needed
                                    cur_state_override = StateOverride::default();
                                    debug!("Block state update received {} {}", block_number, block_hash);
                                    break;
                            }
                        }
                    }
                }
            }
            msg = mempool_events_receiver.recv() => {
                if let Ok(msg) = msg {
                    let mempool_event_msg : MempoolEvents = msg;
                    if let MempoolEvents::MempoolActualTxUpdate{ tx_hash }  = mempool_event_msg {
                        if cur_block_number.is_none() {
                            warn!("Did not received block header update yet!");
                            continue;
                        }

                        tokio::task::spawn(
                            pending_tx_state_change_task(
                                client.clone(),
                                tx_hash,
                                market.clone(),
                                mempool.clone(),
                                market_state.clone(),
                                affecting_tx.clone(),
                                cur_block_number.unwrap_or_default(),
                                cur_block_time.unwrap_or_default(),
                                cur_next_base_fee,
                                cur_state_override.clone(),
                                state_updates_broadcaster.clone(),
                            )
                        );
                    }
                }
            }
        }
    }
}

pub struct PendingTxStateChangeProcessorComponent<P, N, DB: Clone + Send + Sync + 'static, LDT: NodePrimitives + 'static> {
    client: P,

    market: Option<Arc<RwLock<Market>>>,

    mempool: Option<Arc<RwLock<Mempool<LDT>>>>,

    market_state: Option<Arc<RwLock<MarketState<DB>>>>,

    market_events_rx: Option<broadcast::Sender<MarketEvents>>,

    mempool_events_rx: Option<broadcast::Sender<MempoolEvents>>,

    state_updates_tx: Option<broadcast::Sender<StateUpdateEvent<DB, LDT>>>,
    _n: PhantomData<N>,
}

impl<P, N, DB, LDT> PendingTxStateChangeProcessorComponent<P, N, DB, LDT>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: NodePrimitives + 'static,
{
    pub fn new(client: P) -> PendingTxStateChangeProcessorComponent<P, N, DB, LDT> {
        PendingTxStateChangeProcessorComponent {
            client,
            market: None,
            mempool: None,
            market_state: None,
            market_events_rx: None,
            mempool_events_rx: None,
            state_updates_tx: None,
            _n: PhantomData,
        }
    }

    pub fn with_channels(
        self,
        market_events_rx: broadcast::Sender<MarketEvents>,
        mempool_events_rx: broadcast::Sender<MempoolEvents>,
        state_updates_tx: broadcast::Sender<StateUpdateEvent<DB, LDT>>,
    ) -> Self {
        Self {
            market_events_rx: Some(market_events_rx),
            mempool_events_rx: Some(mempool_events_rx),
            state_updates_tx: Some(state_updates_tx),
            ..self
        }
    }

    pub fn with_market(self, market: Arc<RwLock<Market>>) -> Self {
        Self { market: Some(market), ..self }
    }

    pub fn with_mempool(self, mempool: Option<Arc<RwLock<Mempool<LDT>>>>) -> Self {
        Self { mempool, ..self }
    }

    pub fn with_market_state(self, market_state: Arc<RwLock<MarketState<DB>>>) -> Self {
        Self { market_state: Some(market_state), ..self }
    }
}

impl<P, N, DB, LDT> Component for PendingTxStateChangeProcessorComponent<P, N, DB, LDT>
where
    N: Network<TransactionRequest = alloy_rpc_types::TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    LDT: NodePrimitives + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let mempool_events_rx = self.mempool_events_rx.ok_or_else(|| eyre!("mempool_events_rx not set"))?;
        let market_events_rx = self.market_events_rx.ok_or_else(|| eyre!("market_events_rx not set"))?;
        let state_updates_tx = self.state_updates_tx.ok_or_else(|| eyre!("state_updates_tx not set"))?;
        let market = self.market.ok_or_else(|| eyre!("market not set"))?;
        let mempool = self.mempool.ok_or_else(|| eyre!("mempool not set"))?;
        let market_state = self.market_state.ok_or_else(|| eyre!("market_state not set"))?;

        executor.spawn_critical(name, async move {
            if let Err(e) = pending_tx_state_change_worker(
                self.client.clone(),
                market,
                mempool,
                market_state,
                mempool_events_rx,
                market_events_rx,
                state_updates_tx,
            )
            .await
            {
                error!("pending_tx_state_change_worker failed: {}", e);
            }
        });

        Ok(())
    }
    fn name(&self) -> &'static str {
        "PendingTxStateChangeProcessorComponent"
    }
}
