use alloy_network::{BlockResponse, Network};
use alloy_provider::Provider;
use eyre::Result;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use kabu_core_components::Component;
use kabu_node_config::NodeBlockComponentConfig;
use kabu_types_blockchain::KabuDataTypes;
use kabu_types_events::{MessageBlock, MessageBlockHeader, MessageBlockLogs, MessageBlockStateUpdate};
use reth_tasks::TaskExecutor;

/// Simplified component that processes blockchain data from Ethereum nodes
#[derive(Clone)]
pub struct BlockProcessingComponent<P, N, LDT: KabuDataTypes + 'static> {
    /// JSON-RPC provider for fetching blockchain data
    client: P,
    /// Configuration for block processing
    config: NodeBlockComponentConfig,
    /// Channel to send new block headers
    block_headers_tx: Option<broadcast::Sender<MessageBlockHeader<LDT>>>,
    /// Channel to send new blocks with transactions
    blocks_tx: Option<broadcast::Sender<MessageBlock<LDT>>>,
    /// Channel to send new block logs
    block_logs_tx: Option<broadcast::Sender<MessageBlockLogs<LDT>>>,
    /// Channel to send block state updates
    state_updates_tx: Option<broadcast::Sender<MessageBlockStateUpdate<LDT>>>,
    /// Phantom data for network type
    _phantom: std::marker::PhantomData<(N, LDT)>,
}

impl<P, N, LDT> BlockProcessingComponent<P, N, LDT>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network + 'static,
    LDT: KabuDataTypes + 'static,
{
    pub fn new(client: P, config: NodeBlockComponentConfig) -> Self {
        Self {
            client,
            config,
            block_headers_tx: None,
            blocks_tx: None,
            block_logs_tx: None,
            state_updates_tx: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_channels(
        mut self,
        block_headers_tx: Option<broadcast::Sender<MessageBlockHeader<LDT>>>,
        blocks_tx: Option<broadcast::Sender<MessageBlock<LDT>>>,
        block_logs_tx: Option<broadcast::Sender<MessageBlockLogs<LDT>>>,
        state_updates_tx: Option<broadcast::Sender<MessageBlockStateUpdate<LDT>>>,
    ) -> Self {
        self.block_headers_tx = block_headers_tx;
        self.blocks_tx = blocks_tx;
        self.block_logs_tx = block_logs_tx;
        self.state_updates_tx = state_updates_tx;
        self
    }

    async fn run(self) -> Result<()> {
        info!("Starting block processing component with config: {:?}", self.config);

        // Start polling for new blocks
        let mut last_block = match self.client.get_block_number().await {
            Ok(block_num) => block_num,
            Err(e) => {
                error!("Failed to get current block number: {}", e);
                return Err(e.into());
            }
        };

        info!("Block processing component starting from block {}", last_block);

        let poll_interval = Duration::from_millis(1000); // 1 second polling

        loop {
            tokio::time::sleep(poll_interval).await;

            match self.client.get_block_number().await {
                Ok(current_block) => {
                    if current_block > last_block {
                        // Process new blocks
                        for block_num in (last_block + 1)..=current_block {
                            if let Err(e) = self.process_block(block_num).await {
                                error!("Error processing block {}: {}", block_num, e);
                            }
                        }
                        last_block = current_block;
                    }
                }
                Err(e) => {
                    error!("Error getting current block number: {}", e);
                }
            }
        }
    }

    async fn process_block(&self, block_num: u64) -> Result<()> {
        // Process block headers
        if self.config.block_header && self.block_headers_tx.is_some() {
            if let Err(e) = self.process_block_header(block_num).await {
                error!("Error processing block header {}: {}", block_num, e);
            }
        }

        // Process full blocks with transactions
        if self.config.block_with_tx && self.blocks_tx.is_some() {
            if let Err(e) = self.process_full_block(block_num).await {
                error!("Error processing full block {}: {}", block_num, e);
            }
        }

        // Process block logs
        if self.config.block_logs && self.block_logs_tx.is_some() {
            if let Err(e) = self.process_block_logs(block_num).await {
                error!("Error processing block logs {}: {}", block_num, e);
            }
        }

        // Process state updates
        if self.config.block_state_update && self.state_updates_tx.is_some() {
            if let Err(e) = self.process_state_update(block_num).await {
                error!("Error processing state update {}: {}", block_num, e);
            }
        }

        debug!("Successfully processed block {}", block_num);
        Ok(())
    }

    async fn process_block_header(&self, block_num: u64) -> Result<()> {
        match self.client.get_block_by_number(block_num.into()).await {
            Ok(Some(block)) => {
                let _header = block.header().clone();
                // Create a simplified header message
                // In a real implementation, this would properly convert the header type
                debug!("Processed block header {}", block_num);
                // let msg = MessageBlockHeader::new(LDT::Header::from(header));
                // if let Some(ref tx) = self.block_headers_tx {
                //     let _ = tx.send(msg);
                // }
            }
            Ok(None) => {
                debug!("Block {} not found", block_num);
            }
            Err(e) => {
                error!("Error fetching block {}: {}", block_num, e);
                return Err(e.into());
            }
        }
        Ok(())
    }

    async fn process_full_block(&self, block_num: u64) -> Result<()> {
        match self.client.get_block_by_number(block_num.into()).await {
            Ok(Some(block)) => {
                debug!("Processed full block {} with {} transactions", block_num, block.transactions().len());
                // let msg = MessageBlock::new(LDT::Block::from(block));
                // if let Some(ref tx) = self.blocks_tx {
                //     let _ = tx.send(msg);
                // }
            }
            Ok(None) => {
                debug!("Block {} not found", block_num);
            }
            Err(e) => {
                error!("Error fetching block {}: {}", block_num, e);
                return Err(e.into());
            }
        }
        Ok(())
    }

    async fn process_block_logs(&self, block_num: u64) -> Result<()> {
        // Get logs for this block
        let filter = alloy_rpc_types::Filter::new().from_block(block_num).to_block(block_num);

        match self.client.get_logs(&filter).await {
            Ok(logs) => {
                debug!("Processed {} logs for block {}", logs.len(), block_num);
                // let header = // get header somehow
                // let msg = MessageBlockLogs::new(header, logs);
                // if let Some(ref tx) = self.block_logs_tx {
                //     let _ = tx.send(msg);
                // }
            }
            Err(e) => {
                error!("Error fetching logs for block {}: {}", block_num, e);
                return Err(e.into());
            }
        }
        Ok(())
    }

    async fn process_state_update(&self, block_num: u64) -> Result<()> {
        // Simplified state update processing
        debug!("Processed state update for block {}", block_num);
        // let state_update = // create state update
        // let msg = MessageBlockStateUpdate::new(state_update);
        // if let Some(ref tx) = self.state_updates_tx {
        //     let _ = tx.send(msg);
        // }
        Ok(())
    }
}

impl<P, N, LDT> Component for BlockProcessingComponent<P, N, LDT>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network + 'static,
    LDT: KabuDataTypes + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        executor.spawn_critical(self.name(), async move {
            if let Err(e) = self.run().await {
                error!("Block processing component failed: {}", e);
            }
        });
        Ok(())
    }
    fn name(&self) -> &'static str {
        "BlockProcessingComponent"
    }
}

/// Builder for BlockProcessingComponent
pub struct BlockProcessingComponentBuilder {
    config: NodeBlockComponentConfig,
}

impl BlockProcessingComponentBuilder {
    pub fn new() -> Self {
        Self { config: NodeBlockComponentConfig::all_enabled() }
    }

    pub fn with_config(mut self, config: NodeBlockComponentConfig) -> Self {
        self.config = config;
        self
    }

    pub fn build<P, N, LDT>(self, client: P) -> BlockProcessingComponent<P, N, LDT>
    where
        P: Provider<N> + Send + Sync + Clone + 'static,
        N: Network + 'static,
        LDT: KabuDataTypes + 'static,
    {
        BlockProcessingComponent::new(client, self.config)
    }
}

impl Default for BlockProcessingComponentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
