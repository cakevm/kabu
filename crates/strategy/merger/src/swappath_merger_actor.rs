use alloy_primitives::{Address, U256};
use alloy_rpc_types::Header;
use eyre::{eyre, Result};
use revm::{Database, DatabaseCommit, DatabaseRef};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error, info};

use kabu_core_actors::{subscribe, Accessor, Actor, ActorResult, Broadcaster, Consumer, Producer, SharedState, WorkerResult};
use kabu_core_actors_macros::{Accessor, Consumer, Producer};
use kabu_core_blockchain::{Blockchain, Strategy};
use kabu_evm_db::KabuDBError;
use kabu_types_entities::{LatestBlock, Swap, SwapStep};
use kabu_types_events::{MarketEvents, MessageSwapCompose, SwapComposeData, SwapComposeMessage};
use revm::context::BlockEnv;

async fn arb_swap_steps_optimizer_task<
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone,
>(
    compose_channel_tx: Broadcaster<MessageSwapCompose<DB>>,
    _state_db: DB,
    header: Header,
    request: SwapComposeData<DB>,
) -> Result<()> {
    debug!("Step Simulation started");

    if let Swap::BackrunSwapSteps((sp0, sp1)) = request.swap {
        let start_time = chrono::Local::now();

        let _block_env =
            BlockEnv { number: U256::from(header.number + 1), timestamp: U256::from(header.timestamp + 12), ..Default::default() };

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
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
>(
    multicaller_address: Address,
    latest_block: SharedState<LatestBlock>,
    market_events_rx: Broadcaster<MarketEvents>,
    compose_channel_rx: Broadcaster<MessageSwapCompose<DB>>,
    compose_channel_tx: Broadcaster<MessageSwapCompose<DB>>,
) -> WorkerResult {
    subscribe!(market_events_rx);
    subscribe!(compose_channel_rx);

    let mut ready_requests: Vec<SwapComposeData<DB>> = Vec::new();

    loop {
        tokio::select! {
            msg = market_events_rx.recv() => {
                let msg : Result<MarketEvents, RecvError> = msg;
                match msg {
                    Ok(event) => {
                        match event {
                            MarketEvents::BlockHeaderUpdate{..} =>{
                                debug!("Cleaning ready requests");
                                ready_requests = Vec::new();
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
            msg = compose_channel_rx.recv() => {
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


                        info!("MessageSwapPathEncodeRequest received. stuffing: {:?} swap: {}", compose_data.tx_compose.stuffing_txs_hashes, compose_data.swap);

                        for req in ready_requests.iter() {

                            let req_swap = match &req.swap {
                                Swap::BackrunSwapLine(path)=>path,
                                _ => continue,
                            };

                            // todo!() mega bundle merge
                            if !compose_data.same_stuffing(&req.tx_compose.stuffing_txs_hashes) {
                                continue
                            };


                            match SwapStep::merge_swap_paths( req_swap.clone(), swap_path.clone(), multicaller_address.into() ){
                                Ok((sp0, sp1)) => {
                                    let latest_block_guard = latest_block.read().await;
                                    let block_header = latest_block_guard.block_header.clone().unwrap();
                                    drop(latest_block_guard);

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
                                                    block_header,
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
                        ready_requests.push(compose_data.clone());
                        ready_requests.sort_by(|r0,r1| r1.swap.arb_profit().cmp(&r0.swap.arb_profit())  )

                    }
                    Err(e)=>{error!("{}",e)}
                }
            }
        }
    }
}

#[derive(Consumer, Producer, Accessor)]
pub struct ArbSwapPathMergerActor<DB: Send + Sync + Clone + 'static> {
    multicaller_address: Address,
    #[accessor]
    latest_block: Option<SharedState<LatestBlock>>,
    #[consumer]
    market_events: Option<Broadcaster<MarketEvents>>,
    #[consumer]
    compose_channel_rx: Option<Broadcaster<MessageSwapCompose<DB>>>,
    #[producer]
    compose_channel_tx: Option<Broadcaster<MessageSwapCompose<DB>>>,
}

impl<DB> ArbSwapPathMergerActor<DB>
where
    DB: DatabaseRef + Send + Sync + Clone + 'static,
{
    pub fn new(multicaller_address: Address) -> ArbSwapPathMergerActor<DB> {
        ArbSwapPathMergerActor {
            multicaller_address,
            latest_block: None,
            market_events: None,
            compose_channel_rx: None,
            compose_channel_tx: None,
        }
    }
    pub fn on_bc(self, bc: &Blockchain, strategy: &Strategy<DB>) -> Self {
        Self {
            latest_block: Some(bc.latest_block()),
            market_events: Some(bc.market_events_channel()),
            compose_channel_tx: Some(strategy.swap_compose_channel()),
            compose_channel_rx: Some(strategy.swap_compose_channel()),
            ..self
        }
    }
}

impl<DB> Actor for ArbSwapPathMergerActor<DB>
where
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static,
{
    fn start(&self) -> ActorResult {
        let task = tokio::task::spawn(arb_swap_path_merger_worker(
            self.multicaller_address,
            self.latest_block.clone().unwrap(),
            self.market_events.clone().unwrap(),
            self.compose_channel_rx.clone().unwrap(),
            self.compose_channel_tx.clone().unwrap(),
        ));
        Ok(vec![task])
    }

    fn name(&self) -> &'static str {
        "ArbSwapPathMergerActor"
    }
}

#[cfg(test)]
mod test {
    use alloy_primitives::{Address, U256};
    use kabu_evm_db::KabuDB;
    use kabu_types_entities::{Swap, SwapAmountType, SwapLine, SwapPath, Token};
    use kabu_types_events::SwapComposeData;
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
