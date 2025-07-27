use kabu_core_components::Component;
use reth_tasks::TaskExecutor;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::RwLock;

use alloy_eips::{BlockId, BlockNumberOrTag};
use alloy_network::Network;
use alloy_primitives::{Address, Bytes, U256};
use alloy_provider::Provider;
use alloy_rpc_types_trace::geth::AccountState;
use eyre::{eyre, Result};

use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_defi_address_book::TokenAddressEth;
use kabu_evm_utils::{BalanceCheater, NWETH};
use kabu_types_blockchain::{GethStateUpdate, KabuDataTypes};
use kabu_types_entities::{AccountNonceAndBalanceState, TxSigners};
use kabu_types_market::MarketState;
use revm::{Database, DatabaseCommit, DatabaseRef};
use tracing::{debug, error, trace};

async fn fetch_account_state<P, N>(client: P, address: Address) -> Result<AccountState>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
{
    let code = client.get_code_at(address).block_id(BlockId::Number(BlockNumberOrTag::Latest)).await.ok();
    let balance = client.get_balance(address).block_id(BlockId::Number(BlockNumberOrTag::Latest)).await.ok();
    let nonce = client.get_transaction_count(address).block_id(BlockId::Number(BlockNumberOrTag::Latest)).await.ok();

    Ok(AccountState { balance, code, nonce, storage: BTreeMap::new() })
}

async fn set_monitor_token_balance(
    account_nonce_balance_state: Option<Arc<RwLock<AccountNonceAndBalanceState>>>,
    owner: Address,
    token: Address,
    balance: U256,
) {
    if let Some(account_nonce_balance) = account_nonce_balance_state {
        debug!("set_monitor_balance {} {} {}", owner, token, balance);
        let mut account_nonce_balance_guard = account_nonce_balance.write().await;
        let entry = account_nonce_balance_guard.get_entry_or_default(owner);
        debug!("set_monitor_balance {:?}", entry);

        entry.add_balance(token, balance);
    }
}

async fn set_monitor_nonce(account_nonce_balance_state: Option<Arc<RwLock<AccountNonceAndBalanceState>>>, owner: Address, nonce: u64) {
    if let Some(account_nonce_balance) = account_nonce_balance_state {
        debug!("set_monitor_nonce {} {}", owner, nonce);
        let mut account_nonce_balance_guard = account_nonce_balance.write().await;
        let entry = account_nonce_balance_guard.get_entry_or_default(owner);
        debug!("set_monitor_nonce {:?}", entry);
        entry.set_nonce(nonce);
    }
}

pub async fn preload_market_state<P, N, DB>(
    client: P,
    copied_accounts_vec: Vec<Address>,
    new_accounts_vec: Vec<(Address, u64, U256, Option<Bytes>)>,
    token_balances_vec: Vec<(Address, Address, U256)>,
    market_state: Arc<RwLock<MarketState<DB>>>,
    account_nonce_balance_state: Option<Arc<RwLock<AccountNonceAndBalanceState>>>,
) -> Result<()>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Database + DatabaseCommit + Send + Sync + Clone + 'static,
{
    let mut market_state_guard = market_state.write().await;

    let mut state: GethStateUpdate = BTreeMap::new();

    for address in copied_accounts_vec {
        trace!("Loading address : {address}");
        let acc_state = fetch_account_state(client.clone(), address).await?;

        set_monitor_token_balance(account_nonce_balance_state.clone(), address, Address::ZERO, acc_state.balance.unwrap_or_default()).await;

        set_monitor_nonce(account_nonce_balance_state.clone(), address, acc_state.nonce.unwrap_or_default()).await;
        trace!("Loaded address : {address} {:?}", acc_state);

        state.insert(address, acc_state);
    }

    for (address, nonce, balance, code) in new_accounts_vec {
        trace!("new_accounts added {} {} {}", address, nonce, balance);
        set_monitor_token_balance(account_nonce_balance_state.clone(), address, NWETH::NATIVE_ADDRESS, balance).await;
        state.insert(address, AccountState { balance: Some(balance), code, nonce: Some(nonce), storage: BTreeMap::new() });
    }

    for (token, owner, balance) in token_balances_vec {
        if token == TokenAddressEth::ETH_NATIVE {
            match state.entry(owner) {
                Entry::Vacant(e) => {
                    e.insert(AccountState { balance: Some(balance), nonce: Some(0), code: None, storage: BTreeMap::new() });
                }
                Entry::Occupied(mut e) => {
                    e.get_mut().balance = Some(balance);
                }
            }
        } else {
            match state.entry(token) {
                Entry::Vacant(e) => {
                    let mut acc_state = fetch_account_state(client.clone(), token).await?;
                    acc_state.storage.insert(BalanceCheater::get_balance_cell(token, owner)?.into(), balance.into());
                    e.insert(acc_state);
                }
                Entry::Occupied(mut e) => {
                    e.get_mut().storage.insert(BalanceCheater::get_balance_cell(token, owner)?.into(), balance.into());
                }
            }
        }

        set_monitor_token_balance(account_nonce_balance_state.clone(), owner, token, balance).await;
    }
    market_state_guard.apply_geth_update(state);

    Ok(())
}

#[allow(dead_code)]
pub struct MarketStatePreloadedOneShotActor<P, N, DB> {
    name: &'static str,
    client: P,
    copied_accounts: Vec<Address>,
    new_accounts: Vec<(Address, u64, U256, Option<Bytes>)>,
    token_balances: Vec<(Address, Address, U256)>,

    market_state: Option<Arc<RwLock<MarketState<DB>>>>,

    account_nonce_balance_state: Option<Arc<RwLock<AccountNonceAndBalanceState>>>,
    _n: PhantomData<N>,
}

#[allow(dead_code)]
impl<P, N, DB> MarketStatePreloadedOneShotActor<P, N, DB>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + DatabaseCommit + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    pub fn new(client: P) -> Self {
        Self {
            name: "MarketStatePreloadedOneShotActor",
            client,
            copied_accounts: Vec::new(),
            new_accounts: Vec::new(),
            token_balances: Vec::new(),
            market_state: None,
            account_nonce_balance_state: None,
            _n: PhantomData,
        }
    }

    pub fn with_name(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    pub fn on_bc<LDT: KabuDataTypes>(self, bc: &Blockchain<LDT>, state: &BlockchainState<DB, LDT>) -> Self {
        Self { account_nonce_balance_state: Some(bc.nonce_and_balance()), market_state: Some(state.market_state_commit()), ..self }
    }

    pub fn with_signers(self, tx_signers: Arc<RwLock<TxSigners>>) -> Self {
        match tx_signers.try_read() {
            Ok(signers) => {
                let mut addresses = self.copied_accounts;
                addresses.extend(signers.get_address_vec());
                Self { copied_accounts: addresses, ..self }
            }
            Err(e) => {
                error!("tx_signers.try_read() {}", e);
                self
            }
        }
    }

    pub fn with_copied_account(self, address: Address) -> Self {
        let mut copied_accounts = self.copied_accounts;
        copied_accounts.push(address);
        Self { copied_accounts, ..self }
    }

    pub fn with_copied_accounts<T: Into<Address>>(self, address_vec: Vec<T>) -> Self {
        let mut copied_accounts = self.copied_accounts;
        let more: Vec<Address> = address_vec.into_iter().map(Into::<Address>::into).collect();
        copied_accounts.extend(more);
        Self { copied_accounts, ..self }
    }

    pub fn with_new_account(self, address: Address, nonce: u64, balance: U256, code: Option<Bytes>) -> Self {
        let mut new_accounts = self.new_accounts;
        new_accounts.push((address, nonce, balance, code));
        Self { new_accounts, ..self }
    }

    pub fn with_market_state(self, market_state: Arc<RwLock<MarketState<DB>>>) -> Self {
        Self { market_state: Some(market_state), ..self }
    }

    pub fn with_token_balance(self, token: Address, owner: Address, balance: U256) -> Self {
        let mut token_balances = self.token_balances;
        token_balances.push((token, owner, balance));
        Self { token_balances, ..self }
    }
}

impl<P, N, DB> Component for MarketStatePreloadedOneShotActor<P, N, DB>
where
    N: Network,
    P: Provider<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Database + DatabaseCommit + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let market_state = self.market_state.clone().ok_or_else(|| eyre!("market_state not set"))?;
        executor.spawn(async move {
            if let Err(e) = preload_market_state(
                self.client.clone(),
                self.copied_accounts.clone(),
                self.new_accounts.clone(),
                self.token_balances.clone(),
                market_state,
                self.account_nonce_balance_state.clone(),
            )
            .await
            {
                tracing::error!("Preload market state failed: {}", e);
            }
        });
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "MarketStatePreloadedOneShotActor"
    }
}
