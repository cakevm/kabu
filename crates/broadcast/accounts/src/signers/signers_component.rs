use eyre::{eyre, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use alloy_network::Network;
use alloy_primitives::{Address, U256};
use alloy_provider::Provider;

use kabu_core_components::Component;
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_entities::{AccountNonceAndBalanceState, TxSigners};
use kabu_types_events::{MessageSwapCompose, SwapComposeMessage};
use kabu_types_swap::Swap;
use reth_tasks::TaskExecutor;
use revm::DatabaseRef;

/// Component that handles transaction signing for MEV bot operations
pub struct SignersComponent<P, N, DB: Send + Sync + Clone + 'static, LDT: KabuDataTypes + 'static = KabuDataTypesEthereum> {
    /// JSON-RPC provider for chain interactions
    client: P,
    /// Transaction signers with private keys
    signers: Arc<RwLock<TxSigners<LDT>>>,
    /// Account state for nonce management
    account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
    /// Channel to receive swap compose messages
    swap_compose_rx: Option<broadcast::Receiver<MessageSwapCompose<DB, LDT>>>,
    /// Channel to send signed transactions
    signed_tx_tx: Option<broadcast::Sender<MessageSwapCompose<DB, LDT>>>,
    /// Gas price buffer for dynamic pricing
    gas_price_buffer: u64,
    /// Phantom data for network type
    _network: std::marker::PhantomData<N>,
}

impl<P, N, DB, LDT> SignersComponent<P, N, DB, LDT>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: KabuDataTypes + 'static,
{
    pub fn new(
        client: P,
        signers: Arc<RwLock<TxSigners<LDT>>>,
        account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
        gas_price_buffer: u64,
    ) -> Self {
        Self {
            client,
            signers,
            account_state,
            swap_compose_rx: None,
            signed_tx_tx: None,
            gas_price_buffer,
            _network: std::marker::PhantomData,
        }
    }

    pub fn with_channels(
        mut self,
        swap_compose_channel: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
        signed_tx_channel: broadcast::Sender<MessageSwapCompose<DB, LDT>>,
    ) -> Self {
        self.swap_compose_rx = Some(swap_compose_channel.subscribe());
        self.signed_tx_tx = Some(signed_tx_channel);
        self
    }

    async fn run(self) -> Result<()> {
        info!("Starting signers component");

        // Validate signers are available
        let signer_count = {
            let signers = self.signers.read().await;
            signers.get_address_vec().len()
        };

        if signer_count == 0 {
            return Err(eyre!("No signers configured for signing component"));
        }

        info!("Signers component ready with {} signers", signer_count);

        // Extract receivers and senders
        let mut swap_compose_rx = self.swap_compose_rx;
        let signed_tx_tx = self.signed_tx_tx;
        let signers = self.signers;
        let account_state = self.account_state;
        let client = self.client;
        let gas_price_buffer = self.gas_price_buffer;

        // Main event loop
        loop {
            tokio::select! {
                swap_compose_msg = recv_swap_compose_msg(&mut swap_compose_rx) => {
                    if let Some(swap_compose) = swap_compose_msg {
                        if let Err(e) = handle_swap_compose_signing(
                            &client,
                            &signers,
                            &account_state,
                            &signed_tx_tx,
                            swap_compose,
                            gas_price_buffer,
                        ).await {
                            error!("Error signing swap compose: {}", e);
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    debug!("Signers component heartbeat");
                }
            }
        }
    }
}

/// Standalone helper function to receive swap compose messages
async fn recv_swap_compose_msg<DB: Send + Sync + Clone + 'static, LDT: KabuDataTypes>(
    swap_compose_rx: &mut Option<broadcast::Receiver<MessageSwapCompose<DB, LDT>>>,
) -> Option<MessageSwapCompose<DB, LDT>> {
    if let Some(ref mut rx) = swap_compose_rx {
        match rx.recv().await {
            Ok(msg) => Some(msg),
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                warn!("Signers missed {} swap compose messages", missed);
                None
            }
            Err(broadcast::error::RecvError::Closed) => {
                error!("Swap compose channel closed");
                None
            }
        }
    } else {
        // No swap compose channel, sleep a bit
        tokio::time::sleep(Duration::from_millis(100)).await;
        None
    }
}

/// Handle signing of a swap compose transaction
async fn handle_swap_compose_signing<P, N, DB, LDT>(
    client: &P,
    signers: &Arc<RwLock<TxSigners<LDT>>>,
    account_state: &Arc<RwLock<AccountNonceAndBalanceState>>,
    signed_tx_tx: &Option<broadcast::Sender<MessageSwapCompose<DB, LDT>>>,
    swap_compose_msg: MessageSwapCompose<DB, LDT>,
    gas_price_buffer: u64,
) -> Result<()>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: KabuDataTypes + 'static,
{
    // Only process messages that are ready for signing
    let swap_compose_data = match &swap_compose_msg.inner {
        SwapComposeMessage::Ready(data) => data,
        _ => {
            debug!("Ignoring non-ready swap compose message");
            return Ok(());
        }
    };

    let swap = &swap_compose_data.swap;

    // Select the best signer based on account state and swap requirements
    let selected_signer = select_signer_for_swap(signers, account_state, swap).await?;

    debug!("Selected signer {} for swap", selected_signer);

    // Get current nonce for the selected signer
    let current_nonce = {
        let state = account_state.read().await;
        state.get_account(&selected_signer).map(|acc| acc.get_nonce()).unwrap_or(0)
    };

    // Get current gas price and apply buffer
    let base_gas_price = match client.get_gas_price().await {
        Ok(price) => price,
        Err(e) => {
            warn!("Failed to fetch gas price, using default: {}", e);
            20_000_000_000u128 // 20 gwei default
        }
    };

    let buffered_gas_price = base_gas_price + (base_gas_price * gas_price_buffer as u128 / 100);

    // Create signed transaction data (placeholder - actual signing logic would be more complex)
    let mut updated_compose_data = swap_compose_data.clone();

    // Update the transaction request with proper nonce and gas price
    updated_compose_data.tx_compose.nonce = current_nonce;
    updated_compose_data.tx_compose.priority_gas_fee = buffered_gas_price as u64;

    // Update nonce in account state
    {
        let mut state = account_state.write().await;
        if let Some(account_data) = state.get_mut_account(&selected_signer) {
            account_data.set_nonce(current_nonce + 1);
            debug!("Updated nonce for {} to {}", selected_signer, current_nonce + 1);
        }
    }

    // Send ready transaction
    if let Some(tx_sender) = signed_tx_tx {
        let ready_msg = MessageSwapCompose::ready(updated_compose_data);

        if let Err(e) = tx_sender.send(ready_msg) {
            warn!("Failed to send ready swap compose: {}", e);
        } else {
            debug!("Successfully processed and queued swap compose transaction");
        }
    }

    Ok(())
}

/// Select the most appropriate signer for a given swap
async fn select_signer_for_swap<LDT: KabuDataTypes>(
    signers: &Arc<RwLock<TxSigners<LDT>>>,
    account_state: &Arc<RwLock<AccountNonceAndBalanceState>>,
    _swap: &Swap,
) -> Result<Address> {
    let signers_guard = signers.read().await;
    let available_signers = signers_guard.get_address_vec();

    if available_signers.is_empty() {
        return Err(eyre!("No signers available"));
    }

    // For now, use simple round-robin selection
    // In a more sophisticated implementation, we could consider:
    // - Account balance requirements
    // - Nonce gaps
    // - Gas price optimization
    // - Transaction urgency

    let state = account_state.read().await;

    // Find signer with sufficient ETH balance
    for &signer in &available_signers {
        if let Some(account_data) = state.get_account(&signer) {
            let eth_balance = account_data.get_eth_balance();
            // Check if account has sufficient ETH for gas (rough estimate: 0.01 ETH minimum)
            if eth_balance > U256::from(10_000_000_000_000_000u128) {
                // 0.01 ETH in wei
                return Ok(signer);
            }
        }
    }

    // If no signer has sufficient balance, return the first one anyway
    // (it will fail at execution time with clearer error)
    Ok(available_signers[0])
}

impl<P, N, DB, LDT> Component for SignersComponent<P, N, DB, LDT>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
    LDT: KabuDataTypes + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        executor.spawn_critical(self.name(), async move {
            if let Err(e) = self.run().await {
                error!("Signers component failed: {}", e);
            }
        });
        Ok(())
    }
    fn name(&self) -> &'static str {
        "SignersComponent"
    }
}

/// Builder for SignersComponent
pub struct SignersComponentBuilder {
    gas_price_buffer: u64,
}

impl SignersComponentBuilder {
    pub fn new() -> Self {
        Self {
            gas_price_buffer: 10, // Default 10% gas price buffer
        }
    }

    pub fn with_gas_price_buffer(mut self, buffer_percent: u64) -> Self {
        self.gas_price_buffer = buffer_percent;
        self
    }

    pub fn build<P, N, DB, LDT>(
        self,
        client: P,
        signers: Arc<RwLock<TxSigners<LDT>>>,
        account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
    ) -> SignersComponent<P, N, DB, LDT>
    where
        P: Provider<N> + Send + Sync + Clone + 'static,
        N: Network + 'static,
        DB: DatabaseRef + Send + Sync + Clone + 'static,
        LDT: KabuDataTypes + 'static,
    {
        SignersComponent::new(client, signers, account_state, self.gas_price_buffer)
    }
}

impl Default for SignersComponentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
