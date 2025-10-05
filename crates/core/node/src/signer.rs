//! Bridge component that converts Ready SwapCompose messages to TxCompose for broadcasting

use eyre::{Result, eyre};
use kabu_core_components::Component;
use kabu_types_events::{MessageSwapCompose, MessageTxCompose, SwapComposeMessage, TxComposeData};
use reth_tasks::TaskExecutor;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

/// Component that bridges SwapCompose Ready messages to TxCompose for the broadcaster
pub struct SignerBridgeComponent<DB: Send + Sync + Clone + 'static> {
    swap_compose_rx: Option<broadcast::Receiver<MessageSwapCompose<DB>>>,
    tx_compose_tx: Option<broadcast::Sender<MessageTxCompose>>,
}

impl<DB: Send + Sync + Clone + 'static> Default for SignerBridgeComponent<DB> {
    fn default() -> Self {
        Self { swap_compose_rx: None, tx_compose_tx: None }
    }
}

impl<DB: Send + Sync + Clone + 'static> SignerBridgeComponent<DB> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_channels(
        mut self,
        swap_compose: broadcast::Sender<MessageSwapCompose<DB>>,
        tx_compose: broadcast::Sender<MessageTxCompose>,
    ) -> Self {
        self.swap_compose_rx = Some(swap_compose.subscribe());
        self.tx_compose_tx = Some(tx_compose);
        self
    }

    async fn run(self) -> Result<()> {
        info!("Starting signer bridge component");

        let mut swap_compose_rx = self.swap_compose_rx.ok_or_else(|| eyre!("swap_compose_rx not set"))?;
        let tx_compose_tx = self.tx_compose_tx.ok_or_else(|| eyre!("tx_compose_tx not set"))?;

        loop {
            match swap_compose_rx.recv().await {
                Ok(msg) => {
                    debug!("Signer bridge received message type: {:?}", std::mem::discriminant(msg.inner()));
                    if let SwapComposeMessage::Ready(ready_data) = msg.inner() {
                        info!("Signer bridge converting Ready message to TxCompose for swap: {}", ready_data.swap);

                        // Convert SwapComposeData to TxComposeData
                        let tx_data = TxComposeData {
                            eoa: ready_data.tx_compose.eoa,
                            signer: ready_data.tx_compose.signer.clone(),
                            nonce: ready_data.tx_compose.nonce,
                            eth_balance: ready_data.tx_compose.eth_balance,
                            value: ready_data.tx_compose.value,
                            gas: ready_data.tx_compose.gas,
                            priority_gas_fee: ready_data.tx_compose.priority_gas_fee,
                            stuffing_txs_hashes: ready_data.tx_compose.stuffing_txs_hashes.clone(),
                            stuffing_txs: ready_data.tx_compose.stuffing_txs.clone(),
                            next_block_number: ready_data.tx_compose.next_block_number,
                            next_block_timestamp: ready_data.tx_compose.next_block_timestamp,
                            next_block_base_fee: ready_data.tx_compose.next_block_base_fee,
                            tx_bundle: ready_data.tx_compose.tx_bundle.clone(),
                            rlp_bundle: ready_data.tx_compose.rlp_bundle.clone(),
                            origin: Some("SignerBridge".to_string()),
                            swap: Some(ready_data.swap.clone()),
                            tips: ready_data.tips,
                        };

                        // Send as broadcast message
                        let tx_msg = MessageTxCompose::broadcast(tx_data);
                        if let Err(e) = tx_compose_tx.send(tx_msg) {
                            warn!("Failed to send to tx_compose: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Swap compose channel error: {}", e);
                    return Err(eyre!("Swap compose channel closed"));
                }
            }
        }
    }
}

impl<DB: Send + Sync + Clone + 'static> Component for SignerBridgeComponent<DB> {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        executor.spawn_critical(self.name(), async move {
            if let Err(e) = self.run().await {
                error!("Signer bridge component failed: {}", e);
            }
        });
        Ok(())
    }

    fn name(&self) -> &'static str {
        "SignerBridgeComponent"
    }
}
