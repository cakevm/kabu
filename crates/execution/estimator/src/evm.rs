use alloy_consensus::TxEnvelope;
use alloy_eips::BlockNumberOrTag;
use alloy_eips::eip2718::Encodable2718;
use alloy_evm::EvmEnv;
use alloy_network::{Ethereum, Network};
use alloy_primitives::{Bytes, TxKind, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{TransactionInput, TransactionRequest};
use eyre::{Result, eyre};
use influxdb::{Timestamp, WriteQuery};
use std::marker::PhantomData;
use tokio::sync::{broadcast, broadcast::error::RecvError};
use tracing::{debug, error, info, warn};

use kabu_evm_utils::{NWETH, evm_access_list};
use kabu_types_swap::{EstimationError, Swap, SwapEncoder};

use kabu_core_components::Component;
use kabu_evm_db::{AlloyDB, DatabaseKabuExt, KabuDBError};
use kabu_evm_utils::evm_env::tx_req_to_env;
use kabu_types_events::{HealthEvent, MessageHealthEvent, MessageSwapCompose, SwapComposeData, SwapComposeMessage, TxComposeData, TxState};
use reth_tasks::TaskExecutor;
use revm::context::BlockEnv;
use revm::context_interface::block::BlobExcessGasAndPrice;
use revm::{Database, DatabaseCommit, DatabaseRef};

async fn estimator_task<N, DB>(
    client: Option<impl Provider<N> + 'static>,
    swap_encoder: impl SwapEncoder,
    estimate_request: SwapComposeData<DB>,
    compose_channel_tx: broadcast::Sender<MessageSwapCompose<DB>>,
    health_monitor_channel_tx: Option<broadcast::Sender<MessageHealthEvent>>,
    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
) -> Result<()>
where
    N: Network,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone + 'static,
{
    info!("Starting estimator_task for swap: {}", estimate_request.swap);

    debug!(
        gas_limit = estimate_request.tx_compose.gas,
        base_fee = NWETH::to_float_gwei(estimate_request.tx_compose.next_block_base_fee as u128),
        gas_cost = NWETH::to_float_wei(estimate_request.gas_cost()),
        stuffing_txs_len = estimate_request.tx_compose.stuffing_txs_hashes.len(),
        "EVM estimation",
    );

    let start_time = chrono::Utc::now();

    let tx_signer = estimate_request.tx_compose.signer.clone().ok_or(eyre!("NO_SIGNER"))?;
    info!("Got signer: {:?}", tx_signer.address());
    let gas_price = estimate_request.tx_compose.priority_gas_fee + estimate_request.tx_compose.next_block_base_fee;

    info!("Encoding swap...");
    let (to, call_value, call_data, _) = swap_encoder.encode(
        estimate_request.swap.clone(),
        estimate_request.tips_pct,
        Some(estimate_request.tx_compose.next_block_number),
        None,
        Some(tx_signer.address()),
        Some(estimate_request.tx_compose.eth_balance),
    )?;

    info!("Swap encoded successfully, creating transaction request with nonce {}", estimate_request.tx_compose.nonce);

    let tx_request = TransactionRequest {
        transaction_type: Some(2),
        chain_id: Some(1),
        from: Some(tx_signer.address()),
        to: Some(TxKind::Call(to)),
        gas: Some(estimate_request.tx_compose.gas),
        value: call_value,
        input: TransactionInput::new(call_data.clone()),
        nonce: Some(estimate_request.tx_compose.nonce),
        max_priority_fee_per_gas: Some(estimate_request.tx_compose.priority_gas_fee as u128),
        max_fee_per_gas: Some(
            estimate_request.tx_compose.next_block_base_fee as u128 + estimate_request.tx_compose.priority_gas_fee as u128,
        ),
        ..TransactionRequest::default()
    };

    info!("Checking for poststate...");
    let Some(mut db) = estimate_request.poststate else {
        error!("StateDB is None - poststate not set!");
        return Err(eyre!("STATE_DB_IS_NONE"));
    };
    info!("Got poststate DB");

    if let Some(client) = client {
        let ext_db = AlloyDB::new(client, BlockNumberOrTag::Latest.into());
        if let Some(ext_db) = ext_db {
            db.with_ext_db(ext_db)
        } else {
            error!("AlloyDB is None");
        }
    }

    let evm_env = EvmEnv {
        block_env: BlockEnv {
            timestamp: U256::from(estimate_request.tx_compose.next_block_timestamp),
            number: U256::from(estimate_request.tx_compose.next_block_number),
            basefee: estimate_request.tx_compose.next_block_base_fee,
            blob_excess_gas_and_price: Some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice: 0 }),
            ..Default::default()
        },
        ..Default::default()
    };

    let tx_env = tx_req_to_env(tx_request);

    info!("Calling evm_access_list...");
    let (gas_used, access_list) = match evm_access_list(&db, &evm_env, tx_env) {
        Ok((gas_used, access_list)) => {
            let pool_id_vec = estimate_request.swap.get_pool_id_vec();

            tokio::task::spawn(async move {
                for pool_id in pool_id_vec {
                    let pool_id_string = format!("{pool_id}");
                    let write_query = WriteQuery::new(Timestamp::from(start_time), "estimation")
                        .add_field("success", 1i64)
                        .add_tag("pool", pool_id_string);

                    if let Some(influxdb_write_channel_tx) = &influxdb_write_channel_tx
                        && let Err(e) = influxdb_write_channel_tx.send(write_query)
                    {
                        error!("Failed to send successful estimation latency to influxdb: {:?}", e);
                    }
                }
            });

            (gas_used, access_list)
        }
        Err(e) => {
            warn!(
                "evm_access_list error for block_number={}, block_timestamp={}, swap={}, err={e}",
                estimate_request.tx_compose.next_block_number, estimate_request.tx_compose.next_block_timestamp, estimate_request.swap
            );
            // simulation has failed but this could be caused by a token / pool with unsupported fee issue
            debug!("evm_access_list error calldata : {} {}", to, call_data);

            if let Some(health_monitor_channel_tx) = &health_monitor_channel_tx
                && let Swap::BackrunSwapLine(swap_line) = estimate_request.swap
                && let Err(e) =
                    health_monitor_channel_tx.send(MessageHealthEvent::new(HealthEvent::SwapLineEstimationError(EstimationError {
                        swap_path: swap_line.path,
                        msg: e.to_string(),
                    })))
            {
                error!("Failed to send message to health monitor channel: {:?}", e);
            }

            info!("Simulation failed, returning early without sending Ready");
            return Ok(());
        }
    };
    let swap = estimate_request.swap.clone();

    if gas_used < 60_000 {
        error!(gas_used, %swap, "Incorrect transaction estimation");
        return Err(eyre!("TRANSACTION_ESTIMATED_INCORRECTLY"));
    }

    let gas_cost = U256::from(gas_used as u128 * gas_price as u128);

    debug!(
        "Swap encode swap={}, tips_pct={:?}, next_block_number={}, gas_cost={}, signer={}",
        estimate_request.swap,
        estimate_request.tips_pct,
        estimate_request.tx_compose.next_block_number,
        gas_cost,
        tx_signer.address()
    );

    let (to, call_value, call_data, tips_vec) = match swap_encoder.encode(
        estimate_request.swap.clone(),
        estimate_request.tips_pct,
        Some(estimate_request.tx_compose.next_block_number),
        Some(gas_cost),
        Some(tx_signer.address()),
        Some(estimate_request.tx_compose.eth_balance),
    ) {
        Ok((to, call_value, call_data, tips_vec)) => (to, call_value, call_data, tips_vec),
        Err(error) => {
            error!(%error, %swap, "swap_encoder.encode");
            return Err(error);
        }
    };

    let tx_request = TransactionRequest {
        transaction_type: Some(2),
        chain_id: Some(1),
        from: Some(tx_signer.address()),
        to: Some(TxKind::Call(to)),
        gas: Some((gas_used * 1500) / 1000),
        value: call_value,
        input: TransactionInput::new(call_data),
        nonce: Some(estimate_request.tx_compose.nonce),
        access_list: Some(access_list),
        max_priority_fee_per_gas: Some(estimate_request.tx_compose.priority_gas_fee as u128),
        max_fee_per_gas: Some(
            estimate_request.tx_compose.priority_gas_fee as u128 + estimate_request.tx_compose.next_block_base_fee as u128,
        ),
        ..TransactionRequest::default()
    };

    let encoded_txes: Vec<TxEnvelope> =
        estimate_request.tx_compose.stuffing_txs.iter().map(|item| TxEnvelope::from(item.clone())).collect();

    let stuffing_txs_rlp: Vec<Bytes> = encoded_txes.into_iter().map(|x| Bytes::from(x.encoded_2718())).collect();

    let mut tx_with_state: Vec<TxState> = stuffing_txs_rlp.into_iter().map(TxState::ReadyForBroadcastStuffing).collect();

    tx_with_state.push(TxState::SignatureRequired(Box::new(tx_request)));

    let total_tips = tips_vec.into_iter().map(|v| v.tips).sum();
    let profit_eth = estimate_request.swap.arb_profit_eth();
    let gas_cost_f64 = NWETH::to_float(gas_cost);
    let tips_f64 = NWETH::to_float(total_tips);
    let profit_eth_f64 = NWETH::to_float(profit_eth);
    let profit_f64 = match estimate_request.swap.get_first_token() {
        Some(token_in) => token_in.to_float(estimate_request.swap.arb_profit()),
        None => profit_eth_f64,
    };

    let swap_for_log = estimate_request.swap.clone();
    let sign_request = MessageSwapCompose::ready(SwapComposeData {
        tx_compose: TxComposeData { tx_bundle: Some(tx_with_state), ..estimate_request.tx_compose },
        poststate: Some(db),
        tips: Some(total_tips + gas_cost),
        ..estimate_request
    });

    info!("Sending Ready message for swap: {}", swap_for_log);
    let result = match compose_channel_tx.send(sign_request) {
        Err(error) => {
            error!(%error, "compose_channel_tx.send");
            Err(eyre!("COMPOSE_CHANNEL_SEND_ERROR"))
        }
        _ => Ok(()),
    };

    let sim_duration = chrono::Utc::now() - start_time;

    info!(
        cost=gas_cost_f64,
        profit=profit_f64,
        tips=tips_f64,
        gas_used,
        %swap,
        duration=sim_duration.num_microseconds().unwrap_or_default(),
        " +++ Simulation successful",
    );

    result
}

async fn estimator_worker<N, DB>(
    client: Option<impl Provider<N> + Clone + 'static>,
    encoder: impl SwapEncoder + Send + Sync + Clone + 'static,
    mut compose_channel_rx: broadcast::Receiver<MessageSwapCompose<DB>>,
    compose_channel_tx: broadcast::Sender<MessageSwapCompose<DB>>,
    health_monitor_channel_tx: Option<broadcast::Sender<MessageHealthEvent>>,
    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
) where
    N: Network,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone + 'static,
{
    loop {
        tokio::select! {
            msg = compose_channel_rx.recv() => {
                let compose_request_msg : Result<MessageSwapCompose<DB>, RecvError> = msg;
                match compose_request_msg {
                    Ok(compose_request) =>{
                        if let SwapComposeMessage::Estimate(estimate_request) = compose_request.inner {
                            info!("Estimator processing Estimate request for swap: {}", estimate_request.swap);
                            let compose_channel_tx_cloned = compose_channel_tx.clone();
                            let encoder_cloned = encoder.clone();
                            let client_cloned = client.clone();
                            let influxdb_channel_tx_cloned = influxdb_write_channel_tx.clone();
                            let health_monitor_channel_tx_cloned = health_monitor_channel_tx.clone();
                            tokio::task::spawn(
                                async move {
                                    match estimator_task(
                                        client_cloned,
                                        encoder_cloned,
                                        estimate_request.clone(),
                                        compose_channel_tx_cloned,
                                        health_monitor_channel_tx_cloned,
                                        influxdb_channel_tx_cloned,
                                    ).await {
                                        Ok(_) => info!("Estimator task completed successfully"),
                                        Err(e) => error!("Error in EVM estimator_task: {:?}", e),
                                    }
                                }
                            );
                        }
                    }
                    Err(e)=>{error!("{e}")}
                }
            }
        }
    }
}

pub struct EvmEstimatorComponent<P, N, E, DB: Clone + Send + Sync + 'static> {
    encoder: E,
    client: Option<P>,
    compose_channel_rx: Option<broadcast::Sender<MessageSwapCompose<DB>>>,
    compose_channel_tx: Option<broadcast::Sender<MessageSwapCompose<DB>>>,
    health_monitor_channel_tx: Option<broadcast::Sender<MessageHealthEvent>>,
    influxdb_write_channel_tx: Option<broadcast::Sender<WriteQuery>>,
    _n: PhantomData<N>,
}

impl<P, N, E, DB> EvmEstimatorComponent<P, N, E, DB>
where
    N: Network,
    P: Provider<Ethereum>,
    E: SwapEncoder + Send + Sync + Clone + 'static,
    DB: DatabaseRef + DatabaseKabuExt + Send + Sync + Clone + 'static,
{
    pub fn new(encoder: E) -> Self {
        Self {
            encoder,
            client: None,
            compose_channel_tx: None,
            compose_channel_rx: None,
            health_monitor_channel_tx: None,
            influxdb_write_channel_tx: None,
            _n: PhantomData::<N>,
        }
    }

    pub fn new_with_provider(encoder: E, client: Option<P>) -> Self {
        Self {
            encoder,
            client,
            compose_channel_tx: None,
            compose_channel_rx: None,
            health_monitor_channel_tx: None,
            influxdb_write_channel_tx: None,
            _n: PhantomData::<N>,
        }
    }

    pub fn with_swap_compose_channel(self, channel: broadcast::Sender<MessageSwapCompose<DB>>) -> Self {
        Self { compose_channel_tx: Some(channel.clone()), compose_channel_rx: Some(channel), ..self }
    }
}

impl<P, N, E, DB> Component for EvmEstimatorComponent<P, N, E, DB>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
    E: SwapEncoder + Clone + Send + Sync + 'static,
    DB: DatabaseRef<Error = KabuDBError> + Database<Error = KabuDBError> + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let compose_channel_rx = self.compose_channel_rx.ok_or_else(|| eyre!("compose_channel_rx not set"))?.subscribe();
        let compose_channel_tx = self.compose_channel_tx.ok_or_else(|| eyre!("compose_channel_tx not set"))?;

        executor.spawn_critical(
            name,
            estimator_worker(
                self.client.clone(),
                self.encoder.clone(),
                compose_channel_rx,
                compose_channel_tx,
                self.health_monitor_channel_tx.clone(),
                self.influxdb_write_channel_tx.clone(),
            ),
        );

        Ok(())
    }
    fn name(&self) -> &'static str {
        "EvmEstimatorComponent"
    }
}
