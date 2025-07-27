use alloy_primitives::Address;
use eyre::Result;
use influxdb::{Timestamp, WriteQuery};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};

use kabu_core_components::Component;
use kabu_defi_address_book::TokenAddressEth;
use kabu_types_events::{HealthEvent, MessageHealthEvent};
use kabu_types_market::{Market, PoolId, PoolProtocol};
use lazy_static::lazy_static;
use reth_tasks::TaskExecutor;

lazy_static! {
    static ref TRUSTED_TOKENS: HashSet<Address> = HashSet::from_iter(vec![
        TokenAddressEth::WETH,
        TokenAddressEth::USDC,
        TokenAddressEth::USDT,
        TokenAddressEth::STETH,
        TokenAddressEth::WSTETH,
        TokenAddressEth::WBTC,
        TokenAddressEth::CRV,
        TokenAddressEth::DAI
    ]);
}

pub async fn pool_health_monitor_worker(
    market: Arc<RwLock<Market>>,
    mut pool_health_monitor_rx: broadcast::Receiver<MessageHealthEvent>,
    influx_channel_tx: Option<broadcast::Sender<WriteQuery>>,
) {
    let mut pool_errors_map: HashMap<PoolId, u32> = HashMap::new();
    //let mut estimate_errors_map: HashMap<u64, u32> = HashMap::new();

    loop {
        tokio::select! {
                    msg = pool_health_monitor_rx.recv() => {

                        let pool_health_update : Result<MessageHealthEvent, RecvError>  = msg;
                        match pool_health_update {
                            Ok(pool_health_message)=>{
                                match pool_health_message.inner {
                                    HealthEvent::SwapLineEstimationError(estimate_error) => {
                                        debug!("SwapPath health_monitor message update: {} path : {}", estimate_error.msg, estimate_error.swap_path);
        //                                let entry = estimate_errors_map.entry(estimate_error.swap_path.get_hash()).or_insert(0);
        //                                *entry += 1;
        //                                if *entry >= 10 {
                                            //info!("Disabling path : swap_path={} msg={} counter={}", estimate_error.swap_path, estimate_error.msg, entry);

                                            let start_time=std::time::Instant::now();
                                            let mut market_guard = market.write().await;
                                            debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.write acquired");
                                            let is_ok = market_guard.set_path_disabled(&estimate_error.swap_path, true);
                                            info!("Disabling path : swap_path={} msg={} hash={:#?} ok={}", estimate_error.swap_path, estimate_error.msg, estimate_error.swap_path.get_hash(),is_ok);



                                            for (idx, pool) in estimate_error.swap_path.pools.iter().enumerate() {
                                                let tokens = [estimate_error.swap_path.tokens[idx].get_address(), estimate_error.swap_path.tokens[idx+1].get_address()];
                                                if pool.get_protocol() == PoolProtocol::UniswapV2Like || pool.get_protocol() == PoolProtocol::UniswapV3Like {
                                                //tokens.iter().any(|token_address| !TRUSTED_TOKENS.contains(token_address) ) {
                                                    let pool_id = pool.get_pool_id();



                                                    //if !market_guard.is_pool_disabled(&pool_id) {
                                                            market_guard.set_pool_disabled(&pool_id, &tokens[0], &tokens[1], true);

                                                            match market_guard.get_pool(&pool_id) {
                                                                Some(pool)=>{
                                                                    info!("Disabling pool: protocol={}, pool_id={}, msg=ESTIMATION_FAILED", pool.get_protocol(),pool_id);

                                                                    let amount_f64 = -2.0f64;
                                                                    let pool_protocol = pool.get_protocol().to_string();
                                                                    let pool_id = pool.get_pool_id().to_string();
                                                                    let influx_channel_clone = influx_channel_tx.clone();

                                                                    if let Err(e) = tokio::time::timeout(
                                                                        Duration::from_secs(1),
                                                                        async move {
                                                                            let start_time_utc =   chrono::Utc::now();

                                                                            let write_query = WriteQuery::new(Timestamp::from(start_time_utc), "pool_disabled")
                                                                                .add_field("message", "ESTIMATION_FAILED")
                                                                                .add_field("amount", amount_f64)
                                                                                .add_tag("id", pool_id)
                                                                                .add_tag("protocol", pool_protocol)
                                                                                .add_tag("token_from", tokens[0].to_string())
                                                                                .add_tag("token_to", tokens[1].to_string());

                                                                            if let Some(tx) = influx_channel_clone {
                                                                                if let Err(e) = tx.send(write_query) {
                                                                                    error!("Failed to failed pool to influxdb: {:?}", e);
                                                                                }
                                                                            }
                                                                        }
                                                                    ).await {
                                                                        error!("Failed to send failed pool info to influxdb: {:?}", e);
                                                                    }
                                                                }
                                                                _=>{
                                                                    error!("Disabled pool missing in market: address={}", pool_id);
                                                                }
                                                            }
                                                        //}
                                                }



                                            }


                                            drop(market_guard);
                                            debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.write released");

        //                                }

                                    }
                                    HealthEvent::PoolSwapError(swap_error)=>{
                                        debug!("Pool health_monitor message update: {:?} {} {} ", swap_error.pool, swap_error.kind, swap_error.amount);
                                        let entry = pool_errors_map.entry(swap_error.pool).or_insert(0);
                                        *entry += 1;
                                        if *entry >= 10 {
                                            let start_time=std::time::Instant::now();
                                            let mut market_guard = market.write().await;
                                            debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.write acquired");

                                            //if !market_guard.is_pool_disabled(&swap_error.pool) {
                                                market_guard.set_pool_disabled(&swap_error.pool, &swap_error.token_from, &swap_error.token_to, true);


                                                match market_guard.get_pool(&swap_error.pool) {
                                                    Some(pool)=>{
                                                        info!("Disabling pool: protocol={}, address={:?}, error={} amount={}", pool.get_protocol(),swap_error.pool, swap_error.kind, swap_error.amount);

                                                        let amount_f64 = if let Some(token_in) = market_guard.get_token(&swap_error.token_from) {
                                                            token_in.to_float(swap_error.amount)
                                                        } else {
                                                            -1.0f64
                                                        };

                                                        let pool_protocol = pool.get_protocol().to_string();
                                                        let pool_id = pool.get_pool_id().to_string();
                                                        let influx_channel_clone = influx_channel_tx.clone();

                                                        if let Err(e) = tokio::time::timeout(
                                                            Duration::from_secs(1),
                                                            async move {
                                                                let start_time_utc =   chrono::Utc::now();

                                                                let write_query = WriteQuery::new(Timestamp::from(start_time_utc), "pool_disabled")
                                                                    .add_field("message", swap_error.kind.to_string())
                                                                    .add_field("amount", amount_f64)
                                                                    .add_tag("id", pool_id)
                                                                    .add_tag("protocol", pool_protocol)
                                                                    .add_tag("token_from", swap_error.token_from.to_string())
                                                                    .add_tag("token_to", swap_error.token_to.to_string());

                                                                if let Some(tx) = influx_channel_clone {
                                                                    if let Err(e) = tx.send(write_query) {
                                                                        error!("Failed to failed pool to influxdb: {:?}", e);
                                                                    }
                                                                }
                                                            }
                                                        ).await {
                                                            error!("Failed to send failed pool info to influxdb: {:?}", e);
                                                        }



                                                    }
                                                    _=>{
                                                        error!("Disabled pool missing in market: address={:?}, error={} amount={}", swap_error.pool, swap_error.kind, swap_error.amount);
                                                    }
                                                }
                                            //}

                                            drop(market_guard);
                                            debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.write released");

                                        }
                                    }
                                    _=>{}
                                }
                            }
                            Err(e)=>{
                                error!("pool_health_update error {}", e)
                            }
                        }

                    }
                }
    }
}

#[derive(Default)]
pub struct PoolHealthMonitorComponent {
    market: Option<Arc<RwLock<Market>>>,
    pool_health_update_rx: Option<broadcast::Sender<MessageHealthEvent>>,
    influxdb_tx: Option<broadcast::Sender<WriteQuery>>,
}

impl PoolHealthMonitorComponent {
    pub fn new() -> Self {
        PoolHealthMonitorComponent::default()
    }

    pub fn with_channels(
        self,
        market: Arc<RwLock<Market>>,
        pool_health_update_rx: broadcast::Sender<MessageHealthEvent>,
        influxdb_tx: Option<broadcast::Sender<WriteQuery>>,
    ) -> Self {
        Self { market: Some(market), pool_health_update_rx: Some(pool_health_update_rx), influxdb_tx }
    }
}

impl Component for PoolHealthMonitorComponent {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let market = self.market.ok_or_else(|| eyre::eyre!("market not set"))?;
        let pool_health_update_rx = self.pool_health_update_rx.ok_or_else(|| eyre::eyre!("pool_health_update_rx not set"))?.subscribe();

        executor.spawn_critical(name, pool_health_monitor_worker(market, pool_health_update_rx, self.influxdb_tx));

        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "PoolHealthMonitorComponent"
    }
}
