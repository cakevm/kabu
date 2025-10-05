use alloy_network::Ethereum;
use alloy_provider::Provider;
use eyre::{Result, eyre};
use futures_util::StreamExt;
use kabu_core_components::Component;
use kabu_evm_db::DatabaseKabuExt;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_blockchain::ChainParameters;
use kabu_types_entities::{BlockHistory, BlockHistoryManager, BlockHistoryState};
use kabu_types_events::MarketEvents;
use kabu_types_market::MarketState;
use reth_chain_state::{CanonStateNotification, CanonStateSubscriptions};
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use reth_tasks::TaskExecutor;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::borrow::BorrowMut;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info, warn};

pub async fn set_chain_head<P, DB>(
    block_history_manager: &BlockHistoryManager<P, Ethereum, DB, EthPrimitives>,
    block_history: &mut BlockHistory<DB, EthPrimitives>,
    market_events_tx: broadcast::Sender<MarketEvents>,
    header: <EthPrimitives as NodePrimitives>::BlockHeader,
    chain_parameters: &ChainParameters,
) -> Result<(bool, usize)>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: BlockHistoryState<EthPrimitives> + Clone,
{
    let block_number = header.number;
    let block_hash = header.hash_slow();

    debug!(%block_number, %block_hash, "set_chain_head block_number");

    match block_history_manager.set_chain_head(block_history, header.clone()).await {
        Ok((is_new_block, reorg_depth)) => {
            if reorg_depth > 0 {
                debug!("Re-org detected. Block {} Depth {} New hash {}", block_number, reorg_depth, block_hash);
            }

            if is_new_block {
                let base_fee = header.base_fee_per_gas.unwrap_or_default();
                let gas_used = header.gas_used;
                let gas_limit = header.gas_limit;
                let next_base_fee = chain_parameters.calc_next_block_base_fee(gas_used, gas_limit, base_fee) as u128;

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

pub async fn new_block_history_worker_canonical<R, P, DB>(
    reth_provider: R,
    client: P,
    chain_parameters: ChainParameters,
    _market_state: Arc<RwLock<MarketState<DB>>>,
    block_history: Arc<RwLock<BlockHistory<DB, EthPrimitives>>>,
    market_events_tx: broadcast::Sender<MarketEvents>,
) -> Result<()>
where
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: BlockHistoryState<EthPrimitives> + DatabaseRef + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone + 'static,
{
    debug!("new_block_history_worker_canonical started");

    let block_history_manager = BlockHistoryManager::new(client);

    // Subscribe to canonical state notifications
    let mut canon_stream = reth_provider.canonical_state_stream();

    info!("Block history worker subscribed to canonical state notifications");

    while let Some(notification) = canon_stream.next().await {
        match notification {
            CanonStateNotification::Reorg { old: _, new } => {
                debug!("Processing reorg with new chain for block history");

                // Process each block in the new chain
                for recovered_block in new.blocks().values() {
                    // RecoveredBlock provides header() method from trait
                    let header = recovered_block.header();
                    let block_number = header.number;
                    let block_hash = header.hash_slow();

                    debug!("Processing reorged block {} {}", block_number, block_hash);

                    let mut block_history_guard = block_history.write().await;

                    // Update chain head
                    match set_chain_head(
                        &block_history_manager,
                        block_history_guard.borrow_mut(),
                        market_events_tx.clone(),
                        header.clone(),
                        &chain_parameters,
                    )
                    .await
                    {
                        Ok(_) => {
                            // Extract the inner block from RecoveredBlock and add to history
                            let block = recovered_block.clone().into_block();
                            if let Err(e) = block_history_guard.add_block(block) {
                                error!("Failed to add reorged block to history: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to set chain head for reorged block: {}", e);
                        }
                    }
                }
            }
            CanonStateNotification::Commit { new } => {
                debug!("Processing committed chain for block history");

                // Process each block in the committed chain
                for recovered_block in new.blocks().values() {
                    // RecoveredBlock provides header() method from trait
                    let header = recovered_block.header();
                    let block_number = header.number;
                    let block_hash = header.hash_slow();

                    debug!("Processing committed block {} {}", block_number, block_hash);

                    let mut block_history_guard = block_history.write().await;

                    // Update chain head
                    match set_chain_head(
                        &block_history_manager,
                        block_history_guard.borrow_mut(),
                        market_events_tx.clone(),
                        header.clone(),
                        &chain_parameters,
                    )
                    .await
                    {
                        Ok((is_new_block, reorg_depth)) => {
                            if reorg_depth > 0 {
                                info!("Reorg detected at block {} depth {}", block_number, reorg_depth);
                            }

                            if is_new_block {
                                // Extract the inner block from RecoveredBlock and add to history
                                let block = recovered_block.clone().into_block();
                                match block_history_guard.add_block(block) {
                                    Ok(_) => {
                                        // Send block update event
                                        if let Err(e) = market_events_tx.send(MarketEvents::BlockTxUpdate { block_number, block_hash }) {
                                            error!("Failed to send block update event: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to add block to history at {} {}: {}", block_number, block_hash, e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to set chain head: {}", e);
                        }
                    }
                }
            }
        }
    }

    warn!("Canonical state stream ended");
    Ok(())
}

#[derive(Clone)]
pub struct BlockHistoryComponent<R, P, DB> {
    reth_provider: R,
    client: P,
    chain_parameters: ChainParameters,
    market_state: Option<Arc<RwLock<MarketState<DB>>>>,
    block_history: Option<Arc<RwLock<BlockHistory<DB, EthPrimitives>>>>,
    market_events_tx: Option<broadcast::Sender<MarketEvents>>,
}

impl<R, P, DB> BlockHistoryComponent<R, P, DB>
where
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Sync + Send + Clone + 'static,
    DB: DatabaseRef
        + BlockHistoryState<EthPrimitives>
        + DatabaseKabuExt
        + DatabaseCommit
        + Database
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
{
    pub fn new(reth_provider: R, client: P) -> Self {
        Self {
            reth_provider,
            client,
            chain_parameters: ChainParameters::ethereum(),
            market_state: None,
            block_history: None,
            market_events_tx: None,
        }
    }

    pub fn with_config(
        mut self,
        chain_parameters: ChainParameters,
        market_state: Arc<RwLock<MarketState<DB>>>,
        block_history: Arc<RwLock<BlockHistory<DB, EthPrimitives>>>,
        market_events_tx: broadcast::Sender<MarketEvents>,
    ) -> Self {
        self.chain_parameters = chain_parameters;
        self.market_state = Some(market_state);
        self.block_history = Some(block_history);
        self.market_events_tx = Some(market_events_tx);
        self
    }
}

impl<R, P, DB> Component for BlockHistoryComponent<R, P, DB>
where
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Sync + Send + Clone + 'static,
    DB: BlockHistoryState<EthPrimitives> + DatabaseRef + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let market_state = self.market_state.ok_or_else(|| eyre!("market_state not set"))?;
        let block_history = self.block_history.ok_or_else(|| eyre!("block_history not set"))?;
        let market_events_tx = self.market_events_tx.ok_or_else(|| eyre!("market_events_tx not set"))?;

        executor.spawn_critical(name, async move {
            if let Err(e) = new_block_history_worker_canonical(
                self.reth_provider,
                self.client,
                self.chain_parameters,
                market_state,
                block_history,
                market_events_tx,
            )
            .await
            {
                error!("Block history worker failed: {}", e);
            }
        });

        Ok(())
    }

    fn name(&self) -> &'static str {
        "BlockHistoryComponent"
    }
}
