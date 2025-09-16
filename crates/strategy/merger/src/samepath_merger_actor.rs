use reth_tasks::TaskExecutor;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use alloy_consensus::Transaction as _;
use alloy_eips::BlockNumberOrTag;
use alloy_network::Network;
use alloy_primitives::{Address, TxHash, U256};
use alloy_provider::Provider;
use alloy_rpc_types::state::StateOverride;
use alloy_rpc_types::{BlockOverrides, TransactionInput, TransactionRequest};
use alloy_rpc_types_trace::geth::GethDebugTracingCallOptions;
use eyre::{Result, eyre};
use kabu_core_blockchain::{Blockchain, BlockchainState, Strategy};
use kabu_core_components::Component;
use kabu_evm_db::{DatabaseHelpers, KabuDBError};
use kabu_evm_utils::evm_env::tx_req_to_env;
use kabu_evm_utils::evm_transact;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_blockchain::{GethStateUpdate, GethStateUpdateVec, TRACING_CALL_OPTS, debug_trace_call_pre_state};
use kabu_types_entities::{DataFetcher, FetchState};
use kabu_types_events::{MarketEvents, MessageSwapCompose, SwapComposeData, SwapComposeMessage, TxComposeData};
use kabu_types_market::MarketState;
use kabu_types_swap::Swap;
use lazy_static::lazy_static;
use reth_node_types::NodePrimitives;
use reth_primitives_traits::{SignedTransaction, SignerRecoverable};
use revm::context::{BlockEnv, ContextTr};
use revm::context_interface::block::BlobExcessGasAndPrice;
use revm::{Context, Database, DatabaseCommit, DatabaseRef, MainBuilder, MainContext};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error, info, trace};

lazy_static! {
    static ref COINBASE: Address = "0x1f9090aaE28b8a3dCeaDf281B0F12828e676c326".parse().unwrap();
}

fn get_merge_list<'a, DB: Clone + 'static>(
    request: &SwapComposeData<DB>,
    swap_paths: &'a HashMap<TxHash, Vec<SwapComposeData<DB>>>,
) -> Vec<&'a SwapComposeData<DB>> {
    //let mut ret : Vec<&TxComposeData> = Vec::new();
    let swap_line = if let Swap::BackrunSwapLine(swap_line) = &request.swap {
        swap_line
    } else {
        return Vec::new();
    };

    let swap_stuffing_hash = request.first_stuffing_hash();

    let mut ret: Vec<&SwapComposeData<DB>> = swap_paths
        .iter()
        .filter_map(|(k, v)| {
            if *k != swap_stuffing_hash {
                v.iter().find(|a| if let Swap::BackrunSwapLine(a_line) = &a.swap { a_line.path == swap_line.path } else { false })
            } else {
                None
            }
        })
        .collect();

    ret.sort_by(|a, b| b.swap.arb_profit_eth().cmp(&a.swap.arb_profit_eth()));

    ret
}

async fn same_path_merger_task<P, N, DB, NP>(
    client: P,
    stuffing_txes: Vec<NP::SignedTx>,
    pre_states: Arc<RwLock<DataFetcher<TxHash, GethStateUpdate>>>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    call_opts: GethDebugTracingCallOptions,
    request: SwapComposeData<DB, NP>,
    swap_request_tx: broadcast::Sender<MessageSwapCompose<DB, NP>>,
) -> Result<()>
where
    N: Network<TransactionRequest = TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: Database<Error = KabuDBError> + DatabaseRef<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
    NP: NodePrimitives,
{
    debug!("same_path_merger_task stuffing_txs len {}", stuffing_txes.len());

    let mut prestate_guard = pre_states.write().await;

    let mut stuffing_state_locks: Vec<(NP::SignedTx, FetchState<GethStateUpdate>)> = Vec::new();

    for tx in stuffing_txes.into_iter() {
        let client_clone = client.clone();
        let tx_hash: TxHash = *tx.tx_hash();
        let call_opts_clone = call_opts.clone();

        // Manually construct TransactionRequest from signed tx
        let from = tx.recover_signer().map_err(|e| eyre!("Failed to recover signer: {}", e))?;
        let tx_request = TransactionRequest::default()
            .from(from)
            .to(tx.to().unwrap_or_default())
            .value(tx.value())
            .input(TransactionInput::new(tx.input().clone()))
            .nonce(tx.nonce())
            .gas_limit(tx.gas_limit())
            .max_fee_per_gas(tx.max_fee_per_gas())
            .max_priority_fee_per_gas(tx.max_priority_fee_per_gas().unwrap_or_default());

        let lock = prestate_guard
            .fetch(tx_hash, |_tx_hash| async move {
                debug_trace_call_pre_state(client_clone, tx_request, BlockNumberOrTag::Latest.into(), Some(call_opts_clone)).await
            })
            .await;

        stuffing_state_locks.push((tx, lock));
    }

    drop(prestate_guard);

    let mut stuffing_states: Vec<(NP::SignedTx, GethStateUpdate)> = Vec::new();

    for (tx, lock) in stuffing_state_locks.into_iter() {
        if let FetchState::Fetching(lock) = lock
            && let Some(t) = lock.read().await.deref()
        {
            stuffing_states.push((tx, t.clone()));
        }
    }

    let mut tx_order: Vec<usize> = (0..stuffing_states.len()).collect();

    let mut changing: Option<usize> = None;
    let mut counter = 0;

    let db_org = market_state.read().await.state_db.clone();

    let rdb: Option<DB> = loop {
        counter += 1;
        if counter > 10 {
            break None;
        }

        let mut ok = true;

        let tx_and_state: Vec<&(NP::SignedTx, GethStateUpdate)> = tx_order.iter().map(|i| stuffing_states.get(*i).unwrap()).collect();

        let states: GethStateUpdateVec = tx_and_state.iter().map(|(_tx, state)| state.clone()).collect();

        let mut db = db_org.clone();

        DatabaseHelpers::apply_geth_state_update_vec(&mut db, states);

        let block_env = BlockEnv {
            number: U256::from(request.tx_compose.next_block_number),
            timestamp: U256::from(request.tx_compose.next_block_timestamp),
            basefee: request.tx_compose.next_block_base_fee,
            blob_excess_gas_and_price: Some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 0 }),
            ..Default::default()
        };

        let mut evm = Context::mainnet().with_db(db).with_block(block_env).build_mainnet();

        for (idx, tx_idx) in tx_order.clone().iter().enumerate() {
            // set tx context for evm
            let tx = &stuffing_states[*tx_idx].0;

            // Manually construct TransactionRequest from signed tx
            let from = match tx.recover_signer() {
                Ok(addr) => addr,
                Err(e) => {
                    error!("Failed to recover signer: {}", e);
                    ok = false;
                    break;
                }
            };
            let tx_req = TransactionRequest::default()
                .from(from)
                .to(tx.to().unwrap_or_default())
                .value(tx.value())
                .input(TransactionInput::new(tx.input().clone()))
                .nonce(tx.nonce())
                .gas_limit(tx.gas_limit())
                .max_fee_per_gas(tx.max_fee_per_gas())
                .max_priority_fee_per_gas(tx.max_priority_fee_per_gas().unwrap_or_default());
            let tx_env = tx_req_to_env(tx_req);

            // TODO: EVM transact functionality is placeholder for now

            match evm_transact(&mut evm, tx_env) {
                Ok(_c) => {
                    trace!("Transaction {} committed successfully", idx);
                }
                Err(e) => {
                    error!("Transaction {} commit error: {}", idx, e);
                    match changing {
                        Some(changing_idx) => {
                            if (changing_idx == idx && idx == 0) || (changing_idx == idx - 1) {
                                tx_order.remove(changing_idx);
                                trace!("Removing Some {idx} {changing_idx}");
                                changing = None;
                            } else if idx < tx_order.len() && idx > 0 {
                                tx_order.swap(idx, idx - 1);
                                trace!("Swapping Some {idx} {changing_idx}");
                                changing = Some(idx - 1)
                            }
                        }
                        None => {
                            if idx > 0 {
                                trace!("Swapping None {idx}");
                                tx_order.swap(idx, idx - 1);
                                changing = Some(idx - 1)
                            } else {
                                trace!("Removing None {idx}");
                                tx_order.remove(0);
                                changing = None
                            }
                        }
                    }
                    ok = false;
                    break;
                }
            }
        }

        if ok {
            debug!("Transaction sequence found {tx_order:?}");
            // TODO: Consume DB properly
            let db = evm.ctx.db_ref().clone();
            break Some(db);
        }
    };

    if tx_order.len() < 2 {
        return Err(eyre!("NOT_MERGED"));
    }

    if let Some(db) = rdb {
        let _block_env = BlockEnv {
            number: U256::from(request.tx_compose.next_block_number),
            timestamp: U256::from(request.tx_compose.next_block_timestamp),
            basefee: request.tx_compose.next_block_base_fee,
            blob_excess_gas_and_price: Some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 0 }),
            ..Default::default()
        };

        if let Swap::BackrunSwapLine(swap_line) = request.swap.clone() {
            let first_token = swap_line.get_first_token().unwrap();
            let _amount_in = first_token.calc_token_value_from_eth(U256::from(10).pow(U256::from(17))).unwrap();

            // TODO: Update optimize_with_in_amount to work with KabuEVMWrapper
            match Ok::<(), eyre::Error>(()) {
                Ok(_r) => {
                    let encode_request = MessageSwapCompose::<DB, NP>::prepare(SwapComposeData {
                        tx_compose: TxComposeData {
                            stuffing_txs_hashes: tx_order.iter().map(|i| *stuffing_states[*i].0.tx_hash()).collect(),
                            stuffing_txs: tx_order.iter().map(|i| stuffing_states[*i].0.clone()).collect(),
                            ..request.tx_compose
                        },
                        swap: Swap::BackrunSwapLine(swap_line.clone()),
                        origin: Some("samepath_merger".to_string()),
                        tips_pct: None,
                        poststate: Some(db),
                        poststate_update: None,
                        ..request
                    });

                    if let Err(e) = swap_request_tx.send(encode_request) {
                        error!("{}", e)
                    }
                    info!("+++ Calculation finished {swap_line}");
                }
                Err(e) => {
                    error!("optimization error : {e:?}")
                }
            }
        }
    }

    trace!("same_path_merger_task stuffing_states len {}", stuffing_states.len());

    Ok(())
}

async fn same_path_merger_worker<
    N: Network<TransactionRequest = TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
>(
    client: P,
    market_state: Arc<RwLock<MarketState<DB>>>,
    mut market_events_rx: broadcast::Receiver<MarketEvents>,
    mut compose_channel_rx: broadcast::Receiver<MessageSwapCompose<DB>>,
    compose_channel_tx: broadcast::Sender<MessageSwapCompose<DB>>,
) {
    let mut swap_paths: HashMap<TxHash, Vec<SwapComposeData<DB>>> = HashMap::new();

    let prestate = Arc::new(RwLock::new(DataFetcher::<TxHash, GethStateUpdate>::new()));

    //let mut affecting_tx: HashMap<TxHash, bool> = HashMap::new();
    //let mut cur_base_fee: u128 = 0;
    let mut cur_next_base_fee: u64 = 0;
    let mut cur_block_number: Option<alloy_primitives::BlockNumber> = None;
    let mut cur_block_time: Option<u64> = None;
    let mut cur_state_override: StateOverride = StateOverride::default();

    loop {
        tokio::select! {
            msg = market_events_rx.recv() => {
                if let Ok(msg) = msg {
                    let market_event_msg : MarketEvents = msg;
                    if let MarketEvents::BlockHeaderUpdate{block_number, block_hash,  base_fee, next_base_fee, timestamp} =  market_event_msg {
                        debug!("Block header update {} {} base_fee {} ", block_number, block_hash, base_fee);
                        cur_block_number = Some( block_number + 1);
                        cur_block_time = Some(timestamp + 12 );
                        cur_next_base_fee = next_base_fee;
                        //cur_base_fee = base_fee;
                        *prestate.write().await = DataFetcher::<TxHash, GethStateUpdate>::new();
                        swap_paths = HashMap::new();

                        let new_block_hash = block_hash;

                        for _counter in 0..5  {
                            if let Ok(MarketEvents::BlockStateUpdate{block_hash}) = market_events_rx.recv().await
                                && new_block_hash == block_hash {
                                    // State override now comes from the market state update itself
                                    // TODO: Get state override from provider if needed
                                    cur_state_override = StateOverride::default();
                                    debug!("Block state update received {} {}", block_number, block_hash);
                                    break;
                            }
                        }
                    }
                }
            }


            msg = compose_channel_rx.recv() => {
                let msg : Result<MessageSwapCompose<DB>, RecvError> = msg;
                match msg {
                    Ok(compose_request)=>{
                        if let SwapComposeMessage::Ready(sign_request) = compose_request.inner()
                            && sign_request.tx_compose.stuffing_txs_hashes.len() == 1
                            && let Swap::BackrunSwapLine( _swap_line ) = &sign_request.swap {
                                    let stuffing_tx_hash = sign_request.first_stuffing_hash();

                                    let requests_vec = get_merge_list(sign_request, &swap_paths);
                                    if !requests_vec.is_empty() {

                                        let mut stuffing_txs = vec![sign_request.tx_compose.stuffing_txs[0].clone()];
                                        stuffing_txs.extend( requests_vec.iter().map(|r| r.tx_compose.stuffing_txs[0].clone() ).collect::<Vec<_>>());
                                        let client_clone = client.clone();
                                        let prestate_clone = prestate.clone();

                                        let call_opts : GethDebugTracingCallOptions = GethDebugTracingCallOptions{
                                            block_overrides : Some(BlockOverrides {
                                                number : Some( U256::from(cur_block_number.unwrap_or_default())),
                                                time : Some(cur_block_time.unwrap_or_default()),
                                                coinbase : Some(*COINBASE),
                                                base_fee : Some(U256::from(cur_next_base_fee)),
                                                ..Default::default()
                                            }),
                                            state_overrides : Some(cur_state_override.clone()),
                                            ..TRACING_CALL_OPTS.clone()
                                        };

                                        tokio::task::spawn(
                                            same_path_merger_task(
                                                client_clone,
                                                stuffing_txs,
                                                prestate_clone,
                                                market_state.clone(),
                                                call_opts,
                                                sign_request.clone(),
                                                compose_channel_tx.clone()
                                            )
                                        );
                                    }
                                    let e = swap_paths.entry(stuffing_tx_hash).or_default();
                                    e.push( sign_request.clone() );
                            }
                    },
                    Err(e)=>{
                        error!("{e}")
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct SamePathMergerComponent<P, N, DB: Send + Sync + Clone + 'static> {
    client: P,
    //encoder: SwapStepEncoder,
    market_state: Option<Arc<RwLock<MarketState<DB>>>>,
    market_events: Option<broadcast::Sender<MarketEvents>>,
    compose_channel_rx: Option<broadcast::Sender<MessageSwapCompose<DB>>>,
    compose_channel_tx: Option<broadcast::Sender<MessageSwapCompose<DB>>>,
    _n: PhantomData<N>,
}

impl<P, N, DB> SamePathMergerComponent<P, N, DB>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + DatabaseRef<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
{
    pub fn new(client: P) -> Self {
        Self { client, market_state: None, market_events: None, compose_channel_rx: None, compose_channel_tx: None, _n: PhantomData }
    }

    pub fn with_market_state(self, market_state: Arc<RwLock<MarketState<DB>>>) -> Self {
        Self { market_state: Some(market_state), ..self }
    }

    pub fn with_market_events_channel(self, market_events: broadcast::Sender<MarketEvents>) -> Self {
        Self { market_events: Some(market_events), ..self }
    }

    pub fn with_compose_channel(self, compose_channel: broadcast::Sender<MessageSwapCompose<DB>>) -> Self {
        Self { compose_channel_rx: Some(compose_channel.clone()), compose_channel_tx: Some(compose_channel), ..self }
    }

    pub fn on_bc<LDT: NodePrimitives>(self, bc: &Blockchain, state: &BlockchainState<DB, LDT>, strategy: &Strategy<DB>) -> Self {
        Self {
            market_state: Some(state.market_state_commit()),
            market_events: Some(bc.market_events_channel()),
            compose_channel_tx: Some(strategy.swap_compose_channel()),
            compose_channel_rx: Some(strategy.swap_compose_channel()),
            ..self
        }
    }
}

impl<P, N, DB> Component for SamePathMergerComponent<P, N, DB>
where
    N: Network<TransactionRequest = TransactionRequest>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let market_events_rx = self.market_events.ok_or_else(|| eyre!("market_events not set"))?.subscribe();
        let compose_channel_rx = self.compose_channel_rx.ok_or_else(|| eyre!("compose_channel_rx not set"))?.subscribe();
        let compose_channel_tx = self.compose_channel_tx.ok_or_else(|| eyre!("compose_channel_tx not set"))?;
        let market_state = self.market_state.ok_or_else(|| eyre!("market_state not set"))?;

        executor.spawn_critical(
            name,
            same_path_merger_worker(self.client.clone(), market_state, market_events_rx, compose_channel_rx, compose_channel_tx),
        );

        Ok(())
    }

    fn name(&self) -> &'static str {
        "SamePathMergerComponent"
    }
}
