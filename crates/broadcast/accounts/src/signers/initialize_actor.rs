use alloy_primitives::{hex, Bytes, B256};
use eyre::eyre;
use tracing::{error, info};

use kabu_core_actors::{Accessor, Actor, ActorResult, SharedState, WorkerResult};
use kabu_core_actors_macros::Accessor;
use kabu_core_blockchain::Blockchain;
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_entities::{AccountNonceAndBalanceState, KeyStore, LoomTxSigner, TxSigners};

/// The one-shot actor adds a new signer to the signers and monitor list after and stops.
#[derive(Accessor)]
pub struct InitializeSignersOneShotBlockingActor<LDT: KabuDataTypes> {
    key: Option<Vec<u8>>,
    #[accessor]
    signers: Option<SharedState<TxSigners<LDT>>>,
    #[accessor]
    monitor: Option<SharedState<AccountNonceAndBalanceState>>,
}

async fn initialize_signers_one_shot_worker(
    key: Vec<u8>,
    signers: SharedState<TxSigners<KabuDataTypesEthereum>>,
    monitor: SharedState<AccountNonceAndBalanceState>,
) -> WorkerResult {
    let new_signer = signers.write().await.add_privkey(Bytes::from(key));
    monitor.write().await.add_account(new_signer.address());
    info!("New signer added {:?}", new_signer.address());
    Ok("Signer added".to_string())
}

impl<LDT: KabuDataTypes> InitializeSignersOneShotBlockingActor<LDT> {
    pub fn new(key: Option<Vec<u8>>) -> InitializeSignersOneShotBlockingActor<LDT> {
        let key = key.unwrap_or_else(|| B256::random().to_vec());

        InitializeSignersOneShotBlockingActor { key: Some(key), signers: None, monitor: None }
    }

    pub fn new_from_encrypted_env() -> InitializeSignersOneShotBlockingActor<LDT> {
        let key = match std::env::var("DATA") {
            Ok(priv_key_enc) => {
                let keystore = KeyStore::new();
                let key = keystore.encrypt_once(hex::decode(priv_key_enc).unwrap().as_slice()).unwrap();
                Some(key)
            }
            _ => None,
        };

        InitializeSignersOneShotBlockingActor { key, signers: None, monitor: None }
    }

    pub fn new_from_encrypted_key(priv_key_enc: Vec<u8>) -> InitializeSignersOneShotBlockingActor<LDT> {
        let keystore = KeyStore::new();
        let key = keystore.encrypt_once(priv_key_enc.as_slice()).unwrap();

        InitializeSignersOneShotBlockingActor { key: Some(key), signers: None, monitor: None }
    }

    pub fn on_bc(self, bc: &Blockchain<LDT>) -> Self {
        Self { monitor: Some(bc.nonce_and_balance()), ..self }
    }

    pub fn with_signers(self, signers: SharedState<TxSigners<LDT>>) -> Self {
        Self { signers: Some(signers), ..self }
    }
}

impl Actor for InitializeSignersOneShotBlockingActor<KabuDataTypesEthereum> {
    fn start_and_wait(&self) -> eyre::Result<()> {
        let key = match self.key.clone() {
            Some(key) => key,
            _ => {
                error!("No signer keys found");
                return Err(eyre!("NO_SIGNER_KEY"));
            }
        };
        let (signers, monitor) = match (self.signers.clone(), self.monitor.clone()) {
            (Some(signers), Some(monitor)) => (signers, monitor),
            _ => {
                error!("Signers or monitor not initialized");
                return Err(eyre!("SIGNERS_OR_MONITOR_NOT_INITIALIZED"));
            }
        };

        let rt = tokio::runtime::Runtime::new()?; // we need a different runtime to wait for the result
        let handle = rt.spawn(async { initialize_signers_one_shot_worker(key, signers, monitor).await });

        self.wait(Ok(vec![handle]))?;
        rt.shutdown_background();

        Ok(())
    }
    fn start(&self) -> ActorResult {
        Err(eyre!("NEED_TO_BE_WAITED"))
    }

    fn name(&self) -> &'static str {
        "InitializeSignersOneShotBlockingActor"
    }
}
