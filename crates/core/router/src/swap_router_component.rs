use eyre::{eyre, Result};
use reth_tasks::TaskExecutor;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};

use kabu_core_components::Component;
use kabu_types_entities::{AccountNonceAndBalanceState, TxSigners};
use kabu_types_events::{MessageSwapCompose, SwapComposeData, SwapComposeMessage, TxComposeData};
use reth_node_types::NodePrimitives;
use revm::DatabaseRef;

/// Swap router component that handles routing of swap requests
pub struct SwapRouterComponent<DB: Send + Sync + Clone + 'static, LDT: NodePrimitives + 'static = reth_ethereum_primitives::EthPrimitives> {
    signers: Arc<RwLock<TxSigners<LDT>>>,
    account_nonce_balance: Arc<RwLock<AccountNonceAndBalanceState>>,
    swap_compose_rx: broadcast::Receiver<MessageSwapCompose<DB, LDT>>,
    swap_compose_tx: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
}

impl<DB, LDT> SwapRouterComponent<DB, LDT>
where
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: NodePrimitives + 'static,
{
    pub fn new(
        signers: Arc<RwLock<TxSigners<LDT>>>,
        account_nonce_balance: Arc<RwLock<AccountNonceAndBalanceState>>,
        swap_compose_tx: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
    ) -> Self {
        let swap_compose_rx = swap_compose_tx.subscribe();
        Self { signers, account_nonce_balance, swap_compose_rx, swap_compose_tx }
    }

    async fn run(mut self) -> Result<()> {
        info!("Starting swap router component");

        loop {
            match self.swap_compose_rx.recv().await {
                Ok(msg) => {
                    if let SwapComposeMessage::Prepare(swap_compose) = msg.inner() {
                        self.handle_encode(swap_compose.clone()).await?;
                    }
                }
                Err(e) => {
                    error!("Swap compose channel error: {}", e);
                    return Err(eyre!("Swap compose channel closed"));
                }
            }
        }
    }

    async fn handle_encode(&self, route_request: SwapComposeData<DB, LDT>) -> Result<()> {
        debug!("Router handling encode request: {}", route_request.swap);

        let signer = match route_request.tx_compose.eoa {
            Some(eoa) => self.signers.read().await.get_signer_by_address(&eoa)?,
            None => self.signers.read().await.get_random_signer().ok_or(eyre!("NO_SIGNER"))?,
        };

        let nonce = self.account_nonce_balance.read().await.get_account(&signer.address()).unwrap().get_nonce();

        let eth_balance = self.account_nonce_balance.read().await.get_account(&signer.address()).unwrap().get_eth_balance();

        if route_request.tx_compose.next_block_base_fee == 0 {
            error!("Block base fee is not set");
            return Err(eyre!("NO_BLOCK_GAS_FEE"));
        }

        let gas = (route_request.swap.pre_estimate_gas()) * 2;

        let estimate_request = SwapComposeData::<DB, LDT> {
            tx_compose: TxComposeData::<LDT> { signer: Some(signer), nonce, eth_balance, gas, ..route_request.tx_compose },
            ..route_request
        };

        let estimate_request = MessageSwapCompose::estimate(estimate_request);

        match self.swap_compose_tx.send(estimate_request) {
            Err(_) => {
                error!("Failed to send estimate request");
                Err(eyre!("ERROR_SENDING_REQUEST"))
            }
            Ok(_) => Ok(()),
        }
    }
}

impl<DB, LDT> Component for SwapRouterComponent<DB, LDT>
where
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: NodePrimitives + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        executor.spawn_critical(self.name(), async move {
            if let Err(e) = self.run().await {
                tracing::error!("Swap router component failed: {}", e);
            }
        });
        Ok(())
    }
    fn name(&self) -> &'static str {
        "SwapRouterComponent"
    }
}

/// Builder for SwapRouterComponent
pub struct SwapRouterComponentBuilder {
    // Builder can be extended with configuration options
}

impl SwapRouterComponentBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub fn build<DB, LDT>(
        self,
        signers: Arc<RwLock<TxSigners<LDT>>>,
        account_nonce_balance: Arc<RwLock<AccountNonceAndBalanceState>>,
        swap_compose_tx: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
    ) -> SwapRouterComponent<DB, LDT>
    where
        DB: DatabaseRef + Send + Sync + Clone + 'static,
        LDT: NodePrimitives + 'static,
    {
        SwapRouterComponent::new(signers, account_nonce_balance, swap_compose_tx)
    }
}

impl Default for SwapRouterComponentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
