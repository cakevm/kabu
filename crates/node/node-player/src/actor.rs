use revm::{Database, DatabaseCommit, DatabaseRef};
use std::any::type_name;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::compose::replayer_compose_worker;
use crate::worker::node_player_worker;
use alloy_network::{Ethereum, Network};
use alloy_primitives::BlockNumber;
use alloy_provider::Provider;
use eyre::Result;
use kabu_core_components::Component;
use kabu_evm_db::{DatabaseKabuExt, KabuDBError};
use kabu_node_debug_provider::DebugProviderExt;
use kabu_types_blockchain::Mempool;
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_events::{MessageBlock, MessageBlockHeader, MessageBlockLogs, MessageBlockStateUpdate, MessageTxCompose};
use kabu_types_market::MarketState;
use reth_tasks::TaskExecutor;
use tokio::sync::{broadcast, RwLock};
use tracing::error;

pub struct NodeBlockPlayerComponent<P, N, DB: Send + Sync + Clone + 'static> {
    client: P,
    start_block: BlockNumber,
    end_block: BlockNumber,
    mempool: Option<Arc<RwLock<Mempool>>>,
    market_state: Option<Arc<RwLock<MarketState<DB>>>>,
    compose_channel: Option<broadcast::Sender<MessageTxCompose<KabuDataTypesEthereum>>>,
    block_header_channel: Option<broadcast::Sender<MessageBlockHeader>>,
    block_with_tx_channel: Option<broadcast::Sender<MessageBlock>>,
    block_logs_channel: Option<broadcast::Sender<MessageBlockLogs>>,
    block_state_update_channel: Option<broadcast::Sender<MessageBlockStateUpdate>>,
    _n: PhantomData<N>,
}

impl<P, N, DB> NodeBlockPlayerComponent<P, N, DB>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: Database<Error = kabu_evm_db::KabuDBError>
        + DatabaseRef<Error = kabu_evm_db::KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + Send
        + Sync
        + Clone
        + 'static,
{
    pub fn new(client: P, start_block: BlockNumber, end_block: BlockNumber) -> NodeBlockPlayerComponent<P, N, DB> {
        NodeBlockPlayerComponent {
            client,
            start_block,
            end_block,
            mempool: None,
            market_state: None,
            compose_channel: None,
            block_header_channel: None,
            block_with_tx_channel: None,
            block_logs_channel: None,
            block_state_update_channel: None,
            _n: PhantomData,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_channels<LDT: KabuDataTypes>(
        self,
        mempool: Arc<RwLock<Mempool>>,
        market_state: Arc<RwLock<MarketState<DB>>>,
        compose_channel: broadcast::Sender<MessageTxCompose<KabuDataTypesEthereum>>,
        block_header_channel: broadcast::Sender<MessageBlockHeader>,
        block_with_tx_channel: broadcast::Sender<MessageBlock>,
        block_logs_channel: broadcast::Sender<MessageBlockLogs>,
        block_state_update_channel: broadcast::Sender<MessageBlockStateUpdate>,
    ) -> Self {
        Self {
            mempool: Some(mempool),
            market_state: Some(market_state),
            compose_channel: Some(compose_channel),
            block_header_channel: Some(block_header_channel),
            block_with_tx_channel: Some(block_with_tx_channel),
            block_logs_channel: Some(block_logs_channel),
            block_state_update_channel: Some(block_state_update_channel),
            ..self
        }
    }
}

impl<P, N, DB> Component for NodeBlockPlayerComponent<P, N, DB>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    N: Send + Sync + 'static,
    DB: Database<Error = KabuDBError> + DatabaseRef<Error = KabuDBError> + DatabaseCommit + DatabaseKabuExt + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        if let Some(mempool) = self.mempool.clone() {
            if let Some(compose_channel) = self.compose_channel.clone() {
                let compose_rx = compose_channel.subscribe();
                executor.spawn_critical("replayer_compose_worker", async move {
                    if let Err(e) = replayer_compose_worker(mempool, compose_rx).await {
                        error!("replayer_compose_worker failed: {}", e);
                    }
                });
            }
        }

        executor.spawn_critical(name, async move {
            if let Err(e) = node_player_worker(
                self.client.clone(),
                self.start_block,
                self.end_block,
                self.mempool.clone(),
                self.market_state.clone(),
                self.block_header_channel.clone(),
                self.block_with_tx_channel.clone(),
                self.block_logs_channel.clone(),
                self.block_state_update_channel.clone(),
            )
            .await
            {
                error!("node_player_worker failed: {}", e);
            }
        });

        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        type_name::<Self>().rsplit("::").next().unwrap_or(type_name::<Self>())
    }
}
