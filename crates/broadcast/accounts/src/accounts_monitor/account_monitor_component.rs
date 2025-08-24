use eyre::Result;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use alloy_network::Network;
use alloy_primitives::{Address, Log, U256};
use alloy_provider::Provider;
use futures_util::StreamExt;
use reth_chain_state::{CanonStateNotification, CanonStateNotificationStream, CanonStateSubscriptions};
use reth_ethereum_primitives::EthPrimitives;

use kabu_core_components::Component;
use kabu_types_blockchain::{KabuBlock, KabuDataTypes, KabuDataTypesEthereum, KabuTx};
use kabu_types_entities::{AccountNonceAndBalanceState, TxSigners};
use kabu_types_events::{MessageBlock, MessageBlockHeader};
use reth_tasks::TaskExecutor;

/// Component that monitors account nonces and balances for managed accounts
#[derive(Clone)]
pub struct AccountMonitorComponent<P, R, N, LDT: KabuDataTypes + 'static = KabuDataTypesEthereum> {
    /// JSON-RPC provider for fetching account data
    client: P,
    /// Reth provider for canonical state subscriptions
    reth_provider: Option<R>,
    /// Shared state containing account nonces and balances
    account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
    /// Signers to monitor accounts for
    signers: Arc<RwLock<TxSigners<LDT>>>,
    /// Channel to receive block headers
    block_header_tx: Option<broadcast::Sender<MessageBlockHeader<LDT>>>,
    /// Channel to receive blocks with transactions
    block_tx: Option<broadcast::Sender<MessageBlock<LDT>>>,
    /// Update interval for fetching account data
    update_interval: Duration,
    /// Phantom data for network type
    _network: std::marker::PhantomData<N>,
}

impl<P, R, N, LDT> AccountMonitorComponent<P, R, N, LDT>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
    N: Network + 'static,
    LDT: KabuDataTypes + 'static,
{
    pub fn new(
        client: P,
        account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
        signers: Arc<RwLock<TxSigners<LDT>>>,
        update_interval: Duration,
    ) -> Self {
        Self {
            client,
            reth_provider: None,
            account_state,
            signers,
            block_header_tx: None,
            block_tx: None,
            update_interval,
            _network: std::marker::PhantomData,
        }
    }

    pub fn with_reth_provider(mut self, reth_provider: R) -> Self {
        self.reth_provider = Some(reth_provider);
        self
    }

    pub fn with_channels(
        mut self,
        block_header_channel: broadcast::Sender<MessageBlockHeader<LDT>>,
        block_channel: broadcast::Sender<MessageBlock<LDT>>,
    ) -> Self {
        self.block_header_tx = Some(block_header_channel);
        self.block_tx = Some(block_channel);
        self
    }

    async fn run(self) -> Result<()> {
        info!("Starting account monitor component");

        // Get initial account list from signers
        let monitored_accounts = self.get_monitored_accounts().await;
        info!("Monitoring {} accounts for nonce and balance updates", monitored_accounts.len());

        // Initialize account state
        self.initialize_accounts(&monitored_accounts).await?;

        // Spawn background task for periodic updates
        let client_clone = self.client.clone();
        let account_state_clone = self.account_state.clone();
        let monitored_accounts_clone = monitored_accounts.clone();
        let update_interval = self.update_interval;

        tokio::spawn(async move {
            periodic_account_update_worker(client_clone, account_state_clone, monitored_accounts_clone, update_interval).await
        });

        // Extract receivers
        let mut block_rx = self.block_tx.map(|tx| tx.subscribe());
        let account_state = self.account_state;

        // Try to subscribe to canonical state notifications if reth provider is available
        let mut canon_stream: Option<CanonStateNotificationStream<EthPrimitives>> = if let Some(ref reth_provider) = self.reth_provider {
            info!("Successfully subscribed to canonical state notifications");
            Some(reth_provider.canonical_state_stream())
        } else {
            warn!("No reth provider configured. Token balance tracking will be disabled.");
            None
        };

        // Main event loop
        loop {
            tokio::select! {
                block_msg = recv_block_msg(&mut block_rx) => {
                    if let Some(block) = block_msg {
                        if let Err(e) = handle_block_update(&account_state, block).await {
                            error!("Error handling block update: {}", e);
                        }
                    }
                }
                canon_notification = async {
                    if let Some(ref mut stream) = canon_stream {
                        stream.next().await
                    } else {
                        futures_util::future::pending().await
                    }
                } => {
                    if let Some(notification) = canon_notification {
                        if let Err(e) = handle_canon_state_notification(&account_state, notification).await {
                            error!("Error handling canonical state notification: {}", e);
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    debug!("Account monitor heartbeat");
                }
            }
        }
    }

    async fn get_monitored_accounts(&self) -> HashSet<Address> {
        let signers = self.signers.read().await;
        signers.get_address_vec().into_iter().collect()
    }

    async fn initialize_accounts(&self, accounts: &HashSet<Address>) -> Result<()> {
        let mut state = self.account_state.write().await;

        for &account in accounts {
            state.add_account(account);

            // Fetch initial nonce and balance
            match self.client.get_transaction_count(account).await {
                Ok(nonce) => {
                    if let Some(account_data) = state.get_mut_account(&account) {
                        account_data.set_nonce(nonce);
                        debug!("Initialized account {} with nonce {}", account, nonce);
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch initial nonce for {}: {}", account, e);
                }
            }

            match self.client.get_balance(account).await {
                Ok(balance) => {
                    if let Some(account_data) = state.get_mut_account(&account) {
                        account_data.set_balance(Address::ZERO, balance); // ETH balance
                        debug!("Initialized account {} with ETH balance {}", account, balance);
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch initial balance for {}: {}", account, e);
                }
            }
        }

        Ok(())
    }
}

/// Standalone helper functions for the main event loop
async fn recv_block_msg<LDT: KabuDataTypes>(block_rx: &mut Option<broadcast::Receiver<MessageBlock<LDT>>>) -> Option<MessageBlock<LDT>> {
    if let Some(ref mut rx) = block_rx {
        match rx.recv().await {
            Ok(msg) => Some(msg),
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                warn!("Account monitor missed {} block messages", missed);
                None
            }
            Err(broadcast::error::RecvError::Closed) => {
                error!("Block channel closed");
                None
            }
        }
    } else {
        // No block channel, sleep a bit
        tokio::time::sleep(Duration::from_millis(100)).await;
        None
    }
}

async fn handle_block_update<LDT: KabuDataTypes>(
    account_state: &Arc<RwLock<AccountNonceAndBalanceState>>,
    block_msg: MessageBlock<LDT>,
) -> Result<()> {
    let block = &block_msg.inner.block;

    // Update nonces based on transactions in this block
    update_nonces_from_block::<LDT>(account_state, block).await?;

    debug!("Updated account nonces from block {}", block.get_header().number);
    Ok(())
}

async fn update_nonces_from_block<LDT: KabuDataTypes>(
    account_state: &Arc<RwLock<AccountNonceAndBalanceState>>,
    block: &LDT::Block,
) -> Result<()> {
    let mut state = account_state.write().await;

    // Process transactions to update nonces
    for tx in block.get_transactions() {
        let from = tx.get_from();

        if state.is_monitored(&from) {
            if let Some(account_data) = state.get_mut_account(&from) {
                let current_nonce = account_data.get_nonce();
                let tx_nonce = tx.get_nonce();

                // Update nonce if this transaction has a higher nonce
                if tx_nonce >= current_nonce {
                    account_data.set_nonce(tx_nonce + 1);
                    debug!("Updated nonce for {} to {} (from tx nonce {})", from, tx_nonce + 1, tx_nonce);
                }
            }
        }
    }

    Ok(())
}

async fn handle_canon_state_notification(
    account_state: &Arc<RwLock<AccountNonceAndBalanceState>>,
    notification: CanonStateNotification<EthPrimitives>,
) -> Result<()> {
    match notification {
        CanonStateNotification::Reorg { old: _, new } => {
            debug!("Processing reorg with new chain");
            process_canonical_chain_logs(account_state, new).await?;
        }
        CanonStateNotification::Commit { new } => {
            debug!("Processing committed chain");
            process_canonical_chain_logs(account_state, new).await?;
        }
    }
    Ok(())
}

async fn process_canonical_chain_logs(
    account_state: &Arc<RwLock<AccountNonceAndBalanceState>>,
    chain: Arc<reth_execution_types::Chain<EthPrimitives>>,
) -> Result<()> {
    // Get receipts from the chain's execution outcome
    let receipts = chain.execution_outcome().receipts();

    // Collect all logs from all receipts
    let mut all_logs = Vec::new();
    for receipt_vec in receipts {
        for receipt in receipt_vec {
            all_logs.extend(receipt.logs.clone());
        }
    }

    if !all_logs.is_empty() {
        // Process the logs to update balances
        update_balances_from_logs(account_state, &all_logs).await?;
        debug!("Updated account balances from {} logs", all_logs.len());
    }

    Ok(())
}

async fn update_balances_from_logs(account_state: &Arc<RwLock<AccountNonceAndBalanceState>>, logs: &[Log]) -> Result<()> {
    let mut state = account_state.write().await;

    // ERC20 Transfer event signature: Transfer(address,address,uint256)
    let transfer_signature = alloy_primitives::keccak256("Transfer(address,address,uint256)");

    for log in logs {
        if log.data.topics().len() >= 3 && log.data.topics()[0] == transfer_signature {
            // Extract from, to, and amount from the Transfer event
            let from = Address::from_word(log.data.topics()[1]);
            let to = Address::from_word(log.data.topics()[2]);
            let token = log.address;

            if log.data.data.len() == 32 {
                let amount = U256::from_be_slice(&log.data.data);

                // Update balances for monitored accounts
                if state.is_monitored(&from) {
                    if let Some(account_data) = state.get_mut_account(&from) {
                        account_data.sub_balance(token, amount);
                        debug!("Subtracted {} of token {} from account {}", amount, token, from);
                    }
                }

                if state.is_monitored(&to) {
                    if let Some(account_data) = state.get_mut_account(&to) {
                        account_data.add_balance(token, amount);
                        debug!("Added {} of token {} to account {}", amount, token, to);
                    }
                }
            }
        }
    }

    Ok(())
}

impl<P, R, N, LDT> Component for AccountMonitorComponent<P, R, N, LDT>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
    N: Network + 'static,
    LDT: KabuDataTypes + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        executor.spawn_critical(self.name(), async move {
            if let Err(e) = self.run().await {
                error!("Account monitor component failed: {}", e);
            }
        });
        Ok(())
    }

    fn name(&self) -> &'static str {
        "AccountMonitorComponent"
    }
}

/// Background worker that periodically fetches account nonces and balances
async fn periodic_account_update_worker<P, N>(
    client: P,
    account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
    monitored_accounts: HashSet<Address>,
    update_interval: Duration,
) -> Result<()>
where
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network + 'static,
{
    info!("Starting periodic account update worker with interval {:?}", update_interval);

    let mut interval = tokio::time::interval(update_interval);

    loop {
        interval.tick().await;

        for &account in &monitored_accounts {
            // Fetch current nonce
            match client.get_transaction_count(account).await {
                Ok(nonce) => {
                    let mut state = account_state.write().await;
                    if let Some(account_data) = state.get_mut_account(&account) {
                        let old_nonce = account_data.get_nonce();
                        if nonce != old_nonce {
                            account_data.set_nonce(nonce);
                            debug!("Updated nonce for {} from {} to {}", account, old_nonce, nonce);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch nonce for {}: {}", account, e);
                }
            }

            // Fetch current ETH balance
            match client.get_balance(account).await {
                Ok(balance) => {
                    let mut state = account_state.write().await;
                    if let Some(account_data) = state.get_mut_account(&account) {
                        let old_balance = account_data.get_eth_balance();
                        if balance != old_balance {
                            account_data.set_balance(Address::ZERO, balance);
                            debug!("Updated ETH balance for {} from {} to {}", account, old_balance, balance);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch balance for {}: {}", account, e);
                }
            }
        }

        debug!("Completed periodic update for {} accounts", monitored_accounts.len());
    }
}

/// Builder for AccountMonitorComponent
pub struct AccountMonitorComponentBuilder {
    update_interval: Duration,
}

impl AccountMonitorComponentBuilder {
    pub fn new() -> Self {
        Self {
            update_interval: Duration::from_secs(30), // Default 30 second update interval
        }
    }

    pub fn with_update_interval(mut self, interval: Duration) -> Self {
        self.update_interval = interval;
        self
    }

    pub fn build<P, R, N, LDT>(
        self,
        client: P,
        account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
        signers: Arc<RwLock<TxSigners<LDT>>>,
    ) -> AccountMonitorComponent<P, R, N, LDT>
    where
        P: Provider<N> + Send + Sync + Clone + 'static,
        R: CanonStateSubscriptions<Primitives = EthPrimitives> + Send + Sync + Clone + 'static,
        N: Network + 'static,
        LDT: KabuDataTypes + 'static,
    {
        AccountMonitorComponent::new(client, account_state, signers, self.update_interval)
    }
}

impl Default for AccountMonitorComponentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
