use alloy_consensus::Transaction;
use alloy_eips::BlockId;
use eyre::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use alloy_network::Network;
use alloy_primitives::utils::format_ether;
use alloy_primitives::Address;
use futures_util::StreamExt;
use kabu_core_components::Component;
use kabu_types_entities::{AccountNonceAndBalanceState, TxSigners};
use reth_chain_state::{CanonStateNotification, CanonStateSubscriptions};
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;
use reth_primitives::Block;
use reth_primitives_traits::{RecoveredBlock, SignerRecoverable};
use reth_provider::StateProviderFactory;
use reth_revm::database::StateProviderDatabase;
use reth_rpc_eth_types::cache::db::StateProviderTraitObjWrapper;
use reth_tasks::TaskExecutor;
use revm::database::CacheDB;
use revm::DatabaseRef;

// TODO: Does this provider has any positive effect on performance?

/// Component that monitors account nonces and balances for managed accounts
#[derive(Clone)]
pub struct AccountMonitorComponent<R, N, P = EthPrimitives>
where
    P: NodePrimitives,
{
    /// Reth provider for canonical state subscriptions
    reth_provider: R,
    /// Shared state containing account nonces and balances
    account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
    /// Signers to monitor accounts for
    signers: Arc<RwLock<TxSigners<P>>>,
    /// Phantom data for network type
    _network: std::marker::PhantomData<N>,
}

impl<R, N, P> AccountMonitorComponent<R, N, P>
where
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + StateProviderFactory + Send + Sync + Clone + 'static,
    N: Network + 'static,
    P: reth_node_types::NodePrimitives + 'static,
{
    pub fn new(reth_provider: R, account_state: Arc<RwLock<AccountNonceAndBalanceState>>, signers: Arc<RwLock<TxSigners<P>>>) -> Self {
        Self { reth_provider, account_state, signers, _network: std::marker::PhantomData }
    }

    async fn run(self) -> Result<()> {
        info!("Starting account monitor component");

        // Get initial account list from signers
        let monitored_accounts = self.get_monitored_accounts().await;
        info!("Monitoring {} accounts for nonce and balance updates", monitored_accounts.len());

        // Initialize account state
        self.initialize_accounts(&monitored_accounts).await?;

        // Subscribe to canonical state stream
        let mut canon_stream = self.reth_provider.canonical_state_stream();

        while let Some(canon_notification) = canon_stream.next().await {
            if let Err(e) = self.handle_canon_state_notification(canon_notification).await {
                error!("Error handling canonical state notification: {}", e);
            }
        }

        Ok(())
    }

    async fn get_monitored_accounts(&self) -> HashSet<Address> {
        let signers = self.signers.read().await;
        signers.get_address_vec().into_iter().collect()
    }

    async fn initialize_accounts(&self, accounts: &HashSet<Address>) -> Result<()> {
        let mut account_state = self.account_state.write().await;

        for &account_address in accounts {
            account_state.add_account(account_address);

            let state = self.reth_provider.state_by_block_id(BlockId::latest())?;
            let db = CacheDB::new(StateProviderDatabase::new(StateProviderTraitObjWrapper(&state)));
            // Fetch initial nonce and balance
            let account = db.basic_ref(account_address)?;

            let Some(account) = account else {
                warn!("Account {} does not exist on chain", account_address);
                continue;
            };

            if let Some(account_data) = account_state.get_mut_account(&account_address) {
                account_data.set_nonce(account.nonce);
                account_data.set_balance(Address::ZERO, account.balance);
                info!(
                    "Initialized account {} with nonce {} and balance {} ETH",
                    account_address,
                    account.nonce,
                    format_ether(account.balance)
                );
            }
        }

        Ok(())
    }

    async fn handle_canon_state_notification(&self, notification: CanonStateNotification<EthPrimitives>) -> Result<()> {
        let new = match notification {
            CanonStateNotification::Reorg { old: _, new } => {
                debug!("Processing reorg with new chain");
                new
            }
            CanonStateNotification::Commit { new } => {
                debug!("Processing committed chain");
                new
            }
        };

        for block in new.blocks().values() {
            update_nonces_from_block(&self.account_state, block).await?;
        }

        Ok(())
    }
}

async fn update_nonces_from_block(account_state: &Arc<RwLock<AccountNonceAndBalanceState>>, block: &RecoveredBlock<Block>) -> Result<()> {
    let mut state = account_state.write().await;

    // Process transactions to update nonces
    for tx in &block.body().transactions {
        let from = tx.recover_signer()?;

        if state.is_monitored(&from) {
            if let Some(account_data) = state.get_mut_account(&from) {
                let current_nonce = account_data.get_nonce();
                let tx_nonce = tx.nonce();

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

impl<R, N, P> Component for AccountMonitorComponent<R, N, P>
where
    R: CanonStateSubscriptions<Primitives = EthPrimitives> + StateProviderFactory + Send + Sync + Clone + 'static,
    N: Network + 'static,
    P: reth_node_types::NodePrimitives + 'static,
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

/// Builder for AccountMonitorComponent
pub struct AccountMonitorComponentBuilder {}

impl AccountMonitorComponentBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub fn build<R, N, P>(
        self,
        reth_provider: R,
        account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
        signers: Arc<RwLock<TxSigners<P>>>,
    ) -> AccountMonitorComponent<R, N, P>
    where
        R: CanonStateSubscriptions<Primitives = EthPrimitives> + StateProviderFactory + Send + Sync + Clone + 'static,
        N: Network + 'static,
        P: NodePrimitives + 'static,
    {
        AccountMonitorComponent::new(reth_provider, account_state, signers)
    }
}

impl Default for AccountMonitorComponentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
