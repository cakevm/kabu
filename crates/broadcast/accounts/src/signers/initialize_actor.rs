use alloy_primitives::{hex, Bytes, B256};
use eyre::eyre;
use kabu_core_components::Component;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use kabu_core_blockchain::Blockchain;
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_entities::{AccountNonceAndBalanceState, KeyStore, LoomTxSigner, TxSigners};

/// The one-shot component adds a new signer to the signers and monitor list after and stops.
pub struct InitializeSignersOneShotBlockingComponent<LDT: KabuDataTypes> {
    key: Option<Vec<u8>>,

    signers: Option<Arc<RwLock<TxSigners<LDT>>>>,

    monitor: Option<Arc<RwLock<AccountNonceAndBalanceState>>>,
}

async fn initialize_signers_one_shot_worker(
    key: Vec<u8>,
    signers: Arc<RwLock<TxSigners<KabuDataTypesEthereum>>>,
    monitor: Arc<RwLock<AccountNonceAndBalanceState>>,
) -> eyre::Result<()> {
    let new_signer = signers.write().await.add_privkey(Bytes::from(key));
    monitor.write().await.add_account(new_signer.address());
    info!("New signer added {:?}", new_signer.address());
    Ok(())
}

impl<LDT: KabuDataTypes> InitializeSignersOneShotBlockingComponent<LDT> {
    pub fn new(key: Option<Vec<u8>>) -> InitializeSignersOneShotBlockingComponent<LDT> {
        let key = key.unwrap_or_else(|| B256::random().to_vec());

        InitializeSignersOneShotBlockingComponent { key: Some(key), signers: None, monitor: None }
    }

    pub fn new_from_encrypted_env() -> InitializeSignersOneShotBlockingComponent<LDT> {
        let key = match std::env::var("DATA") {
            Ok(priv_key_enc) => {
                let keystore = KeyStore::new();
                let key = keystore.encrypt_once(hex::decode(priv_key_enc).unwrap().as_slice()).unwrap();
                Some(key)
            }
            _ => None,
        };

        InitializeSignersOneShotBlockingComponent { key, signers: None, monitor: None }
    }

    pub fn new_from_encrypted_key(priv_key_enc: Vec<u8>) -> InitializeSignersOneShotBlockingComponent<LDT> {
        let keystore = KeyStore::new();
        let key = keystore.encrypt_once(priv_key_enc.as_slice()).unwrap();

        InitializeSignersOneShotBlockingComponent { key: Some(key), signers: None, monitor: None }
    }

    pub fn on_bc(self, bc: &Blockchain<LDT>) -> Self {
        Self { monitor: Some(bc.nonce_and_balance()), ..self }
    }

    pub fn with_signers(self, signers: Arc<RwLock<TxSigners<LDT>>>) -> Self {
        Self { signers: Some(signers), ..self }
    }

    pub fn with_monitor(self, monitor: Arc<RwLock<AccountNonceAndBalanceState>>) -> Self {
        Self { monitor: Some(monitor), ..self }
    }
}

impl Component for InitializeSignersOneShotBlockingComponent<KabuDataTypesEthereum> {
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> eyre::Result<()> {
        let name = self.name();
        let key = self.key.ok_or_else(|| eyre!("No signer keys found"))?;
        let signers = self.signers.ok_or_else(|| eyre!("Signers not initialized"))?;
        let monitor = self.monitor.ok_or_else(|| eyre!("Monitor not initialized"))?;

        executor.spawn_critical(name, async move {
            if let Err(e) = initialize_signers_one_shot_worker(key, signers, monitor).await {
                tracing::error!("Initialize signers failed: {}", e);
            }
        });
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: reth_tasks::TaskExecutor) -> eyre::Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "InitializeSignersOneShotBlockingComponent"
    }
}
