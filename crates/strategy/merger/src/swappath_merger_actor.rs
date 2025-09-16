use alloy_consensus::BlockHeader;
use alloy_primitives::{Address, U256};
use eyre::{Result, eyre};
use kabu_core_components::Component;
use reth_node_types::{HeaderTy, NodeTypesWithDB};
use reth_provider::{BlockNumReader, HeaderProvider};
use reth_tasks::TaskExecutor;
use revm::{Database, DatabaseCommit, DatabaseRef};
use tokio::sync::{broadcast, broadcast::error::RecvError};
use tracing::{debug, error};

use kabu_core_blockchain::{Blockchain, Strategy};
use kabu_evm_db::KabuDBError;
use kabu_types_events::{MarketEvents, MessageSwapCompose, SwapComposeData, SwapComposeMessage};
use kabu_types_swap::{Swap, SwapStep};
use revm::context::BlockEnv;
use revm::context_interface::block::BlobExcessGasAndPrice;

async fn arb_swap_steps_optimizer_task<
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone,
>(
    compose_channel_tx: broadcast::Sender<MessageSwapCompose<DB>>,
    _state_db: DB,
    block_number: u64,
    block_timestamp: u64,
    request: SwapComposeData<DB>,
) -> Result<()> {
    debug!("Step Simulation started");

    if let Swap::BackrunSwapSteps((sp0, sp1)) = request.swap {
        let start_time = chrono::Local::now();

        let _block_env = BlockEnv {
            number: U256::from(block_number + 1),
            timestamp: U256::from(block_timestamp + 12),
            blob_excess_gas_and_price: Some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 0 }),
            ..Default::default()
        };

        // TODO: Update optimize_swap_steps to work with KabuEVMWrapper
        match Ok::<(SwapStep, SwapStep), eyre::Error>((sp0.clone(), sp1.clone())) {
            Ok((s0, s1)) => {
                let encode_request = MessageSwapCompose::prepare(SwapComposeData {
                    origin: Some("merger_searcher".to_string()),
                    tips_pct: None,
                    swap: Swap::BackrunSwapSteps((s0, s1)),
                    ..request
                });
                compose_channel_tx.send(encode_request).map_err(|_| eyre!("CANNOT_SEND"))?;
            }
            Err(e) => {
                error!("Optimization error:{}", e);
                return Err(eyre!("OPTIMIZATION_ERROR"));
            }
        }
        debug!("Step Optimization finished {} + {} {}", &sp0, &sp1, chrono::Local::now() - start_time);
    } else {
        error!("Incorrect swap_type");
        return Err(eyre!("INCORRECT_SWAP_TYPE"));
    }

    Ok(())
}

async fn arb_swap_path_merger_worker<
    N: NodeTypesWithDB,
    R: BlockNumReader + HeaderProvider<Header = HeaderTy<N>> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
>(
    provider: R,
    multicaller_address: Address,
    market_events_rx: broadcast::Sender<MarketEvents>,
    compose_channel_rx: broadcast::Sender<MessageSwapCompose<DB>>,
    compose_channel_tx: broadcast::Sender<MessageSwapCompose<DB>>,
) -> Result<()>
where
    HeaderTy<N>: BlockHeader,
{
    let mut market_events_receiver = market_events_rx.subscribe();
    let mut compose_channel_receiver = compose_channel_rx.subscribe();

    let mut ready_requests: Vec<SwapComposeData<DB>> = Vec::new();
    let mut processed_swaps = std::collections::HashSet::new();

    loop {
        tokio::select! {
            msg = market_events_receiver.recv() => {
                let msg : Result<MarketEvents, RecvError> = msg;
                match msg {
                    Ok(event) => {
                        match event {
                            MarketEvents::BlockHeaderUpdate{..} =>{
                                debug!("Cleaning ready requests");
                                ready_requests = Vec::new();
                                processed_swaps.clear();
                            }
                            MarketEvents::BlockStateUpdate{..}=>{
                                debug!("State updated");
                                //state_db = market_state.read().await.state_db.clone();
                            }
                            _=>{}
                        }
                    }
                    Err(e)=>{error!("{}", e)}
                }

            },
            msg = compose_channel_receiver.recv() => {
                let msg : Result<MessageSwapCompose<DB>, RecvError> = msg;
                match msg {
                    Ok(swap) => {

                        let compose_data = match swap.inner() {
                            SwapComposeMessage::Ready(data) => data,
                            _=>continue,
                        };

                        let swap_path = match &compose_data.swap {
                            Swap::BackrunSwapLine(path) => path,
                            _=>continue,
                        };

                        // Create a unique key for this swap to detect duplicates
                        let swap_key = format!("{:?}", compose_data.swap);

                        // Skip if we've already processed this exact swap
                        if processed_swaps.contains(&swap_key) {
                            debug!("Skipping duplicate swap in merger");
                            continue;
                        }

                        for req in ready_requests.iter() {

                            let req_swap = match &req.swap {
                                Swap::BackrunSwapLine(path)=>path,
                                _ => continue,
                            };

                            // todo!() mega bundle merge
                            if !compose_data.same_stuffing(&req.tx_compose.stuffing_txs_hashes) {
                                continue
                            };

                            // Pre-filter: check if paths can be merged before attempting expensive merge
                            if !SwapStep::can_merge_swap_paths(req_swap, swap_path) {
                                continue;
                            }

                            match SwapStep::merge_swap_paths( req_swap.clone(), swap_path.clone(), multicaller_address ){
                                Ok((sp0, sp1)) => {
                                    // Get latest block number and timestamp from provider
                                    let (block_number, block_timestamp) = match provider.last_block_number() {
                                        Ok(block_num) => {
                                            match provider.sealed_header(block_num) {
                                                Ok(Some(sealed)) => {
                                                    let header = sealed.header();
                                                    (header.number(), header.timestamp())
                                                },
                                                Ok(None) => {
                                                    error!("No header for block {}", block_num);
                                                    continue;
                                                }
                                                Err(e) => {
                                                    error!("Failed to get header for block {}: {}", block_num, e);
                                                    continue;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to get last block number: {}", e);
                                            continue;
                                        }
                                    };

                                    let request = SwapComposeData{
                                        swap : Swap::BackrunSwapSteps((sp0,sp1)),
                                        ..compose_data.clone()
                                    };

                                    if let Some(db) = compose_data.poststate.clone() {
                                        let db_clone = db.clone();

                                        let compose_channel_clone = compose_channel_tx.clone();
                                        tokio::task::spawn( async move {
                                                arb_swap_steps_optimizer_task(
                                                    compose_channel_clone,
                                                    db_clone,
                                                    block_number,
                                                    block_timestamp,
                                                    request
                                                ).await
                                        });
                                    }
                                    break; // only first
                                }
                                Err(e)=>{
                                    error!("SwapPath merge error : {} {}", ready_requests.len(), e);
                                }
                            }
                        }
                        processed_swaps.insert(swap_key);
                        ready_requests.push(compose_data.clone());
                        ready_requests.sort_by(|r0,r1| r1.swap.arb_profit().cmp(&r0.swap.arb_profit())  );

                        // Keep only top 20 most profitable swaps to prevent O(n²) comparison explosion
                        if ready_requests.len() > 20 {
                            ready_requests.truncate(20);
                        }

                    }
                    Err(e)=>{error!("{}",e)}
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct ArbSwapPathMergerComponent<N, R, DB>
where
    N: NodeTypesWithDB,
    R: BlockNumReader + HeaderProvider<Header = HeaderTy<N>> + Send + Sync + Clone + 'static,
    DB: Send + Sync + Clone + 'static,
{
    provider: R,
    multicaller_address: Address,
    market_events: Option<broadcast::Sender<MarketEvents>>,
    compose_channel_rx: Option<broadcast::Sender<MessageSwapCompose<DB>>>,
    compose_channel_tx: Option<broadcast::Sender<MessageSwapCompose<DB>>>,
    _phantom: std::marker::PhantomData<N>,
}

impl<N, R, DB> ArbSwapPathMergerComponent<N, R, DB>
where
    N: NodeTypesWithDB,
    R: BlockNumReader + HeaderProvider<Header = HeaderTy<N>> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
{
    pub fn new(provider: R, multicaller_address: Address) -> ArbSwapPathMergerComponent<N, R, DB> {
        ArbSwapPathMergerComponent {
            provider,
            multicaller_address,
            market_events: None,
            compose_channel_rx: None,
            compose_channel_tx: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_market_events_channel(self, market_events: broadcast::Sender<MarketEvents>) -> Self {
        Self { market_events: Some(market_events), ..self }
    }

    pub fn with_compose_channel(self, compose_channel: broadcast::Sender<MessageSwapCompose<DB>>) -> Self {
        Self { compose_channel_rx: Some(compose_channel.clone()), compose_channel_tx: Some(compose_channel), ..self }
    }

    pub fn on_bc(self, bc: &Blockchain, strategy: &Strategy<DB>) -> Self {
        Self {
            market_events: Some(bc.market_events_channel()),
            compose_channel_tx: Some(strategy.swap_compose_channel()),
            compose_channel_rx: Some(strategy.swap_compose_channel()),
            ..self
        }
    }
}

impl<N, R, DB> Component for ArbSwapPathMergerComponent<N, R, DB>
where
    N: NodeTypesWithDB,
    R: BlockNumReader + HeaderProvider<Header = HeaderTy<N>> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
    HeaderTy<N>: BlockHeader,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let compose_channel_rx = self.compose_channel_rx.ok_or_else(|| eyre!("compose_channel_rx not set"))?;
        let compose_channel_tx = self.compose_channel_tx.ok_or_else(|| eyre!("compose_channel_tx not set"))?;
        let market_events_rx = self.market_events.ok_or_else(|| eyre!("market_events not set"))?;

        executor.spawn_critical(name, async move {
            if let Err(e) = arb_swap_path_merger_worker::<N, _, _>(
                self.provider,
                self.multicaller_address,
                market_events_rx,
                compose_channel_rx,
                compose_channel_tx,
            )
            .await
            {
                error!("Arb swap path merger worker failed: {}", e);
            }
        });

        Ok(())
    }

    fn name(&self) -> &'static str {
        "ArbSwapPathMergerComponent"
    }
}

#[cfg(test)]
mod test {
    use alloy_primitives::{Address, U256};
    use kabu_evm_db::KabuDB;
    use kabu_types_events::SwapComposeData;
    use kabu_types_market::SwapPath;
    use kabu_types_market::Token;
    use kabu_types_swap::{Swap, SwapAmountType, SwapLine};
    use std::sync::Arc;

    #[test]
    pub fn test_sort() {
        let mut ready_requests: Vec<SwapComposeData<KabuDB>> = Vec::new();
        let token = Arc::new(Token::new(Address::random()));

        let sp0 = SwapLine {
            path: SwapPath {
                tokens: vec![token.clone(), token.clone()],
                pools: vec![],
                disabled: false,
                disabled_pool: Default::default(),
                score: None,
            },
            amount_in: SwapAmountType::Set(U256::from(1)),
            amount_out: SwapAmountType::Set(U256::from(2)),
            ..Default::default()
        };
        ready_requests.push(SwapComposeData { swap: Swap::BackrunSwapLine(sp0), ..SwapComposeData::default() });

        let sp1 = SwapLine {
            path: SwapPath {
                tokens: vec![token.clone(), token.clone()],
                pools: vec![],
                disabled: false,
                disabled_pool: Default::default(),
                score: None,
            },
            amount_in: SwapAmountType::Set(U256::from(10)),
            amount_out: SwapAmountType::Set(U256::from(20)),
            ..Default::default()
        };
        ready_requests.push(SwapComposeData { swap: Swap::BackrunSwapLine(sp1), ..SwapComposeData::default() });

        let sp2 = SwapLine {
            path: SwapPath {
                tokens: vec![token.clone(), token.clone()],
                pools: vec![],
                disabled: false,
                disabled_pool: Default::default(),
                score: None,
            },
            amount_in: SwapAmountType::Set(U256::from(3)),
            amount_out: SwapAmountType::Set(U256::from(5)),
            ..Default::default()
        };
        ready_requests.push(SwapComposeData { swap: Swap::BackrunSwapLine(sp2), ..SwapComposeData::default() });

        ready_requests.sort_by(|a, b| a.swap.arb_profit().cmp(&b.swap.arb_profit()));

        assert_eq!(ready_requests[0].swap.arb_profit(), U256::from(1));
        assert_eq!(ready_requests[1].swap.arb_profit(), U256::from(2));
        assert_eq!(ready_requests[2].swap.arb_profit(), U256::from(10));
    }
}
