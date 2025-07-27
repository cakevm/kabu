use kabu_core_components::Component;
use reth_tasks::TaskExecutor;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use alloy_eips::BlockNumberOrTag;
use alloy_network::Ethereum;
use alloy_provider::Provider;
use chrono::{DateTime, Duration, Local};
use eyre::Result;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;
use tracing::{error, info, warn};

use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_evm_db::DatabaseKabuExt;
use kabu_types_blockchain::KabuDataTypes;
use kabu_types_events::{MarketEvents, MessageTxCompose, TxComposeMessageType};
use kabu_types_market::{MarketState, PoolId};
use revm::DatabaseRef;

async fn verify_pool_state_task<P: Provider<Ethereum> + 'static, DB: DatabaseKabuExt>(
    client: P,
    address: PoolId,
    market_state: Arc<RwLock<MarketState<DB>>>,
) -> Result<()> {
    info!("Verifying state {address:?}");
    let address = match address {
        PoolId::Address(addr) => addr,
        PoolId::B256(_) => return Err(eyre::eyre!("B256 pool ID not supported for verification")),
    };
    let account = market_state.write().await.state_db.load_account(address).cloned()?;
    let read_only_cell_hash_set = market_state.read().await.config.read_only_cells.get(&address).cloned().unwrap_or_default();

    for (cell, current_value) in account.storage.iter() {
        if read_only_cell_hash_set.contains(cell) {
            continue;
        }
        match client.get_storage_at(address, *cell).block_id(BlockNumberOrTag::Latest.into()).await {
            Ok(actual_value) => {
                if actual_value.is_zero() {
                    continue;
                }
                if *current_value != actual_value {
                    warn!("verify : account storage is different : {address:?} {cell:?} {current_value:#32x} -> {actual_value:#32x} storage size : {}", account.storage.len());
                    if let Err(e) = market_state.write().await.state_db.insert_account_storage(address, *cell, actual_value) {
                        error!("{e}");
                    }
                }
            }
            Err(e) => {
                error!("Cannot read storage {:?} {:?} : {}", account, cell, e)
            }
        }
    }

    Ok(())
}

pub async fn state_health_monitor_worker<
    P: Provider<Ethereum> + Clone + 'static,
    DB: DatabaseRef + DatabaseKabuExt + Send + Sync + Clone + 'static,
>(
    client: P,
    market_state: Arc<RwLock<MarketState<DB>>>,
    tx_compose_channel_rx: broadcast::Sender<MessageTxCompose>,
    market_events_rx: broadcast::Sender<MarketEvents>,
) -> Result<()> {
    let mut tx_compose_channel_rx: Receiver<MessageTxCompose> = tx_compose_channel_rx.subscribe();
    let mut market_events_rx: Receiver<MarketEvents> = market_events_rx.subscribe();

    let mut check_time_map: HashMap<PoolId, DateTime<Local>> = HashMap::new();
    let mut pool_address_to_verify_vec: Vec<PoolId> = Vec::new();

    loop {
        tokio::select! {
            msg = market_events_rx.recv() => {
                let market_event_msg : Result<MarketEvents, RecvError> = msg;
                match market_event_msg {
                    Ok(market_event)=>{
                        if matches!(market_event, MarketEvents::BlockStateUpdate{..}) {
                            for pool_address in pool_address_to_verify_vec {
                                tokio::task::spawn(
                                    verify_pool_state_task(
                                        client.clone(),
                                        pool_address,
                                        market_state.clone()
                                    )
                                );
                            }
                            pool_address_to_verify_vec = Vec::new();
                        }
                    }
                    Err(e)=>{error!("market_event_rx error : {e}")}
                }
            },

            msg = tx_compose_channel_rx.recv() => {
                let tx_compose_update : Result<MessageTxCompose, RecvError>  = msg;
                match tx_compose_update {
                    Ok(tx_compose_msg)=>{
                        if let TxComposeMessageType::Sign(sign_request_data)= tx_compose_msg.inner {
                            if let Some(swap) = sign_request_data.swap {
                                let pool_address_vec =  swap.get_pool_address_vec();
                                let now = chrono::Local::now();
                                for pool_address in pool_address_vec {
                                    if now - check_time_map.get(&pool_address).cloned().unwrap_or(DateTime::<Local>::MIN_UTC.into()) > Duration::seconds(60) {
                                        check_time_map.insert(pool_address, Local::now());
                                        if !pool_address_to_verify_vec.contains(&pool_address){
                                            pool_address_to_verify_vec.push(pool_address)
                                        }
                                    }
                                }

                            }
                        }

                    }
                    Err(e)=>{
                        error!("tx_compose_channel_rx : {e}")
                    }
                }

            }

        }
    }
}

pub struct StateHealthMonitorActor<P, DB: Clone + Send + Sync + 'static> {
    client: P,

    market_state: Option<Arc<RwLock<MarketState<DB>>>>,

    tx_compose_channel_rx: Option<broadcast::Sender<MessageTxCompose>>,

    market_events_rx: Option<broadcast::Sender<MarketEvents>>,
}

impl<P, DB> StateHealthMonitorActor<P, DB>
where
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + DatabaseKabuExt + Send + Sync + Clone + Default + 'static,
{
    pub fn new(client: P) -> Self {
        StateHealthMonitorActor { client, market_state: None, tx_compose_channel_rx: None, market_events_rx: None }
    }

    pub fn on_bc<LDT: KabuDataTypes>(self, bc: &Blockchain, state: &BlockchainState<DB, LDT>) -> Self {
        Self {
            market_state: Some(state.market_state()),
            tx_compose_channel_rx: Some(bc.tx_compose_channel()),
            market_events_rx: Some(bc.market_events_channel()),
            ..self
        }
    }
}

impl<P, DB> Component for StateHealthMonitorActor<P, DB>
where
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + DatabaseKabuExt + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let market_state = self.market_state.clone().ok_or_else(|| eyre::eyre!("market_state not set"))?;
        let tx_compose_channel_rx = self.tx_compose_channel_rx.clone().ok_or_else(|| eyre::eyre!("tx_compose_channel_rx not set"))?;
        let market_events_rx = self.market_events_rx.clone().ok_or_else(|| eyre::eyre!("market_events_rx not set"))?;

        executor.spawn(async move {
            if let Err(e) = state_health_monitor_worker(self.client.clone(), market_state, tx_compose_channel_rx, market_events_rx).await {
                tracing::error!("State health monitor worker failed: {}", e);
            }
        });
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "StateHealthMonitorActor"
    }
}
