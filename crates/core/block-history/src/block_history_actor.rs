use alloy_json_rpc::RpcRecv;
use alloy_network::{BlockResponse, Network};
use alloy_primitives::{BlockHash, BlockNumber};
use alloy_provider::Provider;
use alloy_rpc_types::Header;
use eyre::{eyre, Result};
use kabu_core_components::Component;
use kabu_evm_db::DatabaseKabuExt;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_blockchain::{ChainParameters, KabuBlock, KabuDataTypes};
use kabu_types_entities::{BlockHistory, BlockHistoryManager, BlockHistoryState};
use kabu_types_events::{MarketEvents, MessageBlock, MessageBlockHeader, MessageBlockLogs, MessageBlockStateUpdate};
use kabu_types_market::MarketState;
use reth_tasks::TaskExecutor;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::borrow::BorrowMut;
use std::marker::PhantomData;
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::sync::{broadcast, broadcast::error::RecvError, RwLock};
use tracing::{debug, error, info, trace, warn};

pub async fn set_chain_head<P, N, DB, LDT>(
    block_history_manager: &BlockHistoryManager<P, N, DB, LDT>,
    block_history: &mut BlockHistory<DB, LDT>,
    market_events_tx: broadcast::Sender<MarketEvents>,
    header: Header,
    chain_parameters: &ChainParameters,
) -> Result<(bool, usize)>
where
    N: Network<BlockResponse = LDT::Block>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: BlockHistoryState<LDT> + Clone,
    LDT: KabuDataTypes,
    LDT::Block: RpcRecv + BlockResponse,
{
    let block_number = header.number;
    let block_hash = header.hash;

    debug!(%block_number, %block_hash, "set_chain_head block_number");

    match block_history_manager.set_chain_head(block_history, header.clone()).await {
        Ok((is_new_block, reorg_depth)) => {
            if reorg_depth > 0 {
                debug!("Re-org detected. Block {} Depth {} New hash {}", block_number, reorg_depth, block_hash);
            }

            if is_new_block {
                let base_fee = header.base_fee_per_gas.unwrap_or_default();
                let next_base_fee = chain_parameters.calc_next_block_base_fee_from_header(&header) as u128;

                let timestamp: u64 = header.timestamp;

                // Block history already tracks the latest block

                if let Err(e) = market_events_tx.send(MarketEvents::BlockHeaderUpdate {
                    block_number,
                    block_hash,
                    timestamp,
                    base_fee,
                    next_base_fee: next_base_fee as u64,
                }) {
                    error!("market_events_tx.send : {}", e);
                }
            }

            Ok((is_new_block, reorg_depth))
        }
        Err(e) => {
            error!("block_history_manager.set_chain_head error at {} hash {} error : {} ", block_number, block_hash, e);
            Err(eyre!("CANNOT_SET_CHAIN_HEAD"))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn new_block_history_worker<P, N, DB, LDT>(
    client: P,
    chain_parameters: ChainParameters,
    market_state: Arc<RwLock<MarketState<DB>>>,
    block_history: Arc<RwLock<BlockHistory<DB, LDT>>>,
    mut block_header_update_rx: broadcast::Receiver<MessageBlockHeader<LDT>>,
    mut block_update_rx: broadcast::Receiver<MessageBlock<LDT>>,
    mut log_update_rx: broadcast::Receiver<MessageBlockLogs<LDT>>,
    mut state_update_rx: broadcast::Receiver<MessageBlockStateUpdate<LDT>>,
    market_events_tx: broadcast::Sender<MarketEvents>,
) -> Result<()>
where
    N: Network<BlockResponse = LDT::Block>,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: BlockHistoryState<LDT> + DatabaseRef + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone + 'static,
    LDT: KabuDataTypes,
    LDT::Block: RpcRecv + BlockResponse,
{
    debug!("new_block_history_worker started");

    let block_history_manager = BlockHistoryManager::<P, N, DB, LDT>::new(client);

    loop {
        tokio::select! {
            msg = block_header_update_rx.recv() => {
                let block_update : Result<MessageBlockHeader<LDT>, RecvError>  = msg;
                match block_update {
                    Ok(block_header)=>{
                        let mut block_history_guard = block_history.write().await;

                        let header = block_header.inner.header.clone();

                        debug!("Block Header, Update {} {}", header.number, header.hash);


                        set_chain_head(
                            &block_history_manager,
                            block_history_guard.borrow_mut(),
                            market_events_tx.clone(),
                            block_header.inner.header,
                            &chain_parameters
                        ).await?;
                    }
                    Err(e)=>{
                        error!("block_update error {}", e)
                    }
                }
            }

            msg = block_update_rx.recv() => {
                let block_update : Result<MessageBlock<LDT>, RecvError>  = msg;
                match block_update {
                    Ok(block)=>{
                        let block = block.inner.block;
                        let block_header = block.get_header();
                        let block_hash : BlockHash = block_header.hash;
                        let block_number : BlockNumber = block_header.number;

                        debug!("Block Update {} {}", block_number, block_hash);

                        let mut block_history_guard = block_history.write().await;

                        match set_chain_head(
                            &block_history_manager,
                            block_history_guard.borrow_mut(),
                            market_events_tx.clone(),
                            block_header,
                            &chain_parameters
                        ).await
                            {
                                Ok(_)=>{
                                    match block_history_guard.add_block(block.clone()) {
                                        Ok(_)=>{
                                            // Send block update event
                                            if let Err(e) = market_events_tx.send(MarketEvents::BlockTxUpdate{ block_number, block_hash}) {
                                                error!("market_events_tx.send : {}", e)
                                            }
                                        }
                                        Err(e)=>{
                                            error!("block_update add_block error at block {} with hash {} : {}", block_number, block_hash, e);
                                        }
                                    }
                                }
                                Err(e)=>{
                                    error!("{}", e);
                                }

                            }
                        }
                    Err(e)=>{
                        error!("block_update error {}", e)
                    }
                }
            }
            msg = log_update_rx.recv() => {
                let log_update : Result<MessageBlockLogs<LDT>, RecvError>  = msg;
                match log_update {
                    Ok(msg) =>{
                        let blocklogs = msg.inner;
                        let block_header = blocklogs.block_header.clone();
                        let block_hash : BlockHash = block_header.hash;
                        let block_number : BlockNumber = block_header.number;

                        debug!("Block Logs Update {} {}", block_number, block_hash);

                        let mut block_history_guard = block_history.write().await;

                        match set_chain_head(
                            &block_history_manager,
                            block_history_guard.borrow_mut(),
                            market_events_tx.clone(),
                            block_header,
                            &chain_parameters
                        ).await
                        {
                            Ok(_)=>{
                                match block_history_guard.add_logs(block_hash,blocklogs.logs.clone()) {
                                    Ok(_)=>{
                                        // Send logs update event
                                        if let Err(e) = market_events_tx.send(MarketEvents::BlockLogsUpdate { block_number, block_hash}) {
                                            error!("market_events_tx.send : {}", e)
                                        }
                                    }
                                    Err(e)=>{
                                        error!("block_logs_update add_logs error at block {} with hash {} : {}", block_number, block_hash, e);
                                    }
                                }
                            }
                            Err(e)=>{
                                error!("block_logs_update {}", e);
                            }
                        }
                    }
                    Err(e)=>{
                        error!("block_update error {}", e)
                    }
                }

            }
            msg = state_update_rx.recv() => {

                let state_update_msg : Result<MessageBlockStateUpdate<LDT>, RecvError> = msg;

                let msg = match state_update_msg {
                    Ok(message_block_state_update) => message_block_state_update,
                    Err(e) => {
                        error!("state_update_rx.recv error {}", e);
                        continue
                    }
                };

                let msg = msg.inner;
                let msg_block_header = msg.block_header;
                let msg_block_number : BlockNumber = msg_block_header.number;
                let msg_block_hash : BlockHash = msg_block_header.hash;
                debug!("Block State update {} {}", msg_block_number, msg_block_hash);


                let mut block_history_guard = block_history.write().await;
                let mut market_state_guard = market_state.write().await;


                if let Err(e) = set_chain_head(&block_history_manager, block_history_guard.borrow_mut(),
                    market_events_tx.clone(), msg_block_header, &chain_parameters).await {
                    error!("set_chain_head : {}", e);
                    continue
                }

                let latest_block_number = block_history_guard.latest_block_number;
                let latest_block_hash = block_history_guard.get_block_hash_for_block_number(latest_block_number).unwrap_or_default();
                let latest_block_entry = block_history_guard.get_block_history_entry(&latest_block_hash);
                let latest_block_parent_hash = latest_block_entry.map(|e| e.parent_hash()).unwrap_or_default();

                if latest_block_hash != msg_block_hash {
                    warn!(%msg_block_number, %msg_block_hash, %latest_block_number, %latest_block_hash, "State update for block that is not latest.");
                    if let Err(err) = block_history_guard.add_state_diff(msg_block_hash,  msg.state_update.clone()) {
                        error!(%err, %msg_block_number, %msg_block_hash, "Error during add_state_diff.");
                    }
                } else{
                    // State update is for the latest block
                    let new_market_state_db = if market_state_guard.block_hash.is_zero() || market_state_guard.block_hash == latest_block_parent_hash {
                         market_state_guard.state_db.clone()
                    } else {
                        match block_history_manager.apply_state_update_on_parent_db(block_history_guard.deref_mut(), &market_state_guard.config, msg_block_hash ).await {
                            Ok(db) => db,
                            Err(err) => {
                                error!(%err, %msg_block_number, %msg_block_hash, "Error during apply_state_update_on_parent_db.");
                                continue
                            }
                        }
                    };


                    if let Err(err) = block_history_guard.add_state_diff(msg_block_hash, msg.state_update.clone()) {
                        error!(%err, %msg_block_number, %msg_block_hash, "Error during block_history.add_state_diff.");
                        continue
                    }

                    let block_history_entry = block_history_guard.get_block_history_entry(&msg_block_hash);

                    let Some(block_history_entry) = block_history_entry else { continue };

                    let updated_db = new_market_state_db.apply_update(block_history_entry, &market_state_guard.config);

                    if let Err(err) = block_history_guard.add_db(msg_block_hash, updated_db.clone()) {
                        error!(%err, %msg_block_number, %msg_block_hash, "Error during block_history.add_db.");
                        continue
                    }

                    debug!("Block History len: {}", block_history_guard.len());

                    let accounts_len = market_state_guard.state_db.accounts_len();
                    let contracts_len = market_state_guard.state_db.contracts_len();
                    let storage_len = market_state_guard.state_db.storage_len();

                    trace!("Market state len accounts {} contracts {} storage {}", accounts_len, contracts_len, storage_len);

                    info!("market state updated ok records : update len: {} accounts: {} contracts: {} storage: {}", msg.state_update.len(),
                         updated_db.accounts_len(), updated_db.contracts_len() , updated_db.storage_len() );

                    market_state_guard.state_db = updated_db.clone();
                    market_state_guard.block_hash = msg_block_hash;
                    market_state_guard.block_number = latest_block_number;


                    match market_events_tx.send(MarketEvents::BlockStateUpdate{ block_hash : msg_block_hash}) {
                        Ok(_) => {},
                        Err(error) => error!(%error, "market_events_tx.send error"),
                    }


                    #[cfg(not(debug_assertions))]
                    {
                        // Merging DB in background and update market state
                        let market_state_clone = market_state.clone();

                        tokio::task::spawn( async move{
                            let merged_db = updated_db.maintain();
                            let mut market_state_guard = market_state_clone.write().await;
                            market_state_guard.state_db = merged_db;
                            debug!("Merged DB stored in MarketState at block {}", msg_block_number)
                        });
                    }

                    #[cfg(debug_assertions)]
                    {

                        market_state_guard.state_db = updated_db.maintain();

                        let accounts = market_state_guard.state_db.accounts_len();

                        let storage = market_state_guard.state_db.storage_len();
                        let contracts = market_state_guard.state_db.contracts_len();

                        trace!(accounts, storage, contracts, "Merging finished. Market state len" );

                    }



                }

            }
        }
    }
}

#[derive(Clone)]
pub struct BlockHistoryComponent<P, N, DB, LDT: KabuDataTypes + 'static> {
    client: P,
    chain_parameters: ChainParameters,
    market_state: Option<Arc<RwLock<MarketState<DB>>>>,
    block_history: Option<Arc<RwLock<BlockHistory<DB, LDT>>>>,
    block_header_update_rx: Option<broadcast::Sender<MessageBlockHeader<LDT>>>,
    block_update_rx: Option<broadcast::Sender<MessageBlock<LDT>>>,
    log_update_rx: Option<broadcast::Sender<MessageBlockLogs<LDT>>>,
    state_update_rx: Option<broadcast::Sender<MessageBlockStateUpdate<LDT>>>,
    market_events_tx: Option<broadcast::Sender<MarketEvents>>,
    _n: PhantomData<N>,
}

impl<P, N, DB, LDT> BlockHistoryComponent<P, N, DB, LDT>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Sync + Send + Clone + 'static,
    DB: DatabaseRef + BlockHistoryState<LDT> + DatabaseKabuExt + DatabaseCommit + Database + Send + Sync + Clone + Default + 'static,
    LDT: KabuDataTypes + 'static,
    LDT::Block: BlockResponse,
{
    pub fn new(client: P) -> Self {
        Self {
            client,
            chain_parameters: ChainParameters::ethereum(),
            market_state: None,
            block_history: None,
            block_header_update_rx: None,
            block_update_rx: None,
            log_update_rx: None,
            state_update_rx: None,
            market_events_tx: None,
            _n: PhantomData,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_channels(
        mut self,
        chain_parameters: ChainParameters,
        market_state: Arc<RwLock<MarketState<DB>>>,
        block_history: Arc<RwLock<BlockHistory<DB, LDT>>>,
        block_header_update_rx: broadcast::Sender<MessageBlockHeader<LDT>>,
        block_update_rx: broadcast::Sender<MessageBlock<LDT>>,
        log_update_rx: broadcast::Sender<MessageBlockLogs<LDT>>,
        state_update_rx: broadcast::Sender<MessageBlockStateUpdate<LDT>>,
        market_events_tx: broadcast::Sender<MarketEvents>,
    ) -> Self {
        self.chain_parameters = chain_parameters;
        self.market_state = Some(market_state);
        self.block_history = Some(block_history);
        self.block_header_update_rx = Some(block_header_update_rx);
        self.block_update_rx = Some(block_update_rx);
        self.log_update_rx = Some(log_update_rx);
        self.state_update_rx = Some(state_update_rx);
        self.market_events_tx = Some(market_events_tx);
        self
    }
}

impl<P, N, DB, LDT> Component for BlockHistoryComponent<P, N, DB, LDT>
where
    N: Network<BlockResponse = LDT::Block>,
    P: Provider<N> + DebugProviderExt<N> + Sync + Send + Clone + 'static,
    DB: BlockHistoryState<LDT> + DatabaseRef + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone + 'static,
    LDT: KabuDataTypes,
    LDT::Block: BlockResponse + RpcRecv,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let block_header_rx = self.block_header_update_rx.ok_or_else(|| eyre!("block_header_update_rx not set"))?.subscribe();
        let block_rx = self.block_update_rx.ok_or_else(|| eyre!("block_update_rx not set"))?.subscribe();
        let log_rx = self.log_update_rx.ok_or_else(|| eyre!("log_update_rx not set"))?.subscribe();
        let state_rx = self.state_update_rx.ok_or_else(|| eyre!("state_update_rx not set"))?.subscribe();

        executor.spawn_critical(name, async move {
            if let Err(e) = new_block_history_worker(
                self.client.clone(),
                self.chain_parameters.clone(),
                self.market_state.clone().unwrap(),
                self.block_history.clone().unwrap(),
                block_header_rx,
                block_rx,
                log_rx,
                state_rx,
                self.market_events_tx.clone().unwrap(),
            )
            .await
            {
                tracing::error!("Block history worker failed: {}", e);
            }
        });

        Ok(())
    }
    fn name(&self) -> &'static str {
        "BlockHistoryComponent"
    }
}
