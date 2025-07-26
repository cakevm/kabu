use alloy_network::Ethereum;
use alloy_primitives::{Address, B256, U256};
use alloy_provider::Provider;
use axum::Router;
use eyre::{eyre, Result};
use kabu_broadcast_accounts::{InitializeSignersOneShotBlockingActor, NonceAndBalanceMonitorActor, TxSignersActor};
use kabu_broadcast_broadcaster::{FlashbotsBroadcastActor, RelayConfig};
use kabu_core_actors::{Actor, ActorsManager, SharedState};
use kabu_core_block_history::BlockHistoryActor;
use kabu_core_blockchain::{Blockchain, BlockchainState, Strategy};
use kabu_core_mempool::MempoolActor;
use kabu_core_router::SwapRouterActor;
use kabu_defi_address_book::TokenAddressEth;
use kabu_defi_health_monitor::{MetricsRecorderActor, PoolHealthMonitorActor, StuffingTxMonitorActor};
use kabu_defi_market::{
    HistoryPoolLoaderOneShotActor, NewPoolLoaderActor, PoolLoaderActor, ProtocolPoolLoaderOneShotActor, RequiredPoolLoaderActor,
};
use kabu_defi_pools::{PoolLoadersBuilder, PoolsLoadingConfig};
use kabu_defi_preloader::MarketStatePreloadedOneShotActor;
use kabu_defi_price::PriceActor;
use kabu_evm_db::{DatabaseKabuExt, KabuDBError};
use kabu_evm_utils::NWETH;
use kabu_execution_estimator::EvmEstimatorActor;
use kabu_execution_multicaller::MulticallerSwapEncoder;
use kabu_metrics::InfluxDbWriterActor;
use kabu_node_actor_config::NodeBlockActorConfig;
use kabu_node_debug_provider::DebugProviderExt;
use kabu_node_json_rpc::{NodeBlockActor, NodeMempoolActor, WaitForNodeSyncOneShotBlockingActor};
use kabu_rpc_handler::WebServerActor;
use kabu_storage_db::DbPool;
use kabu_strategy_backrun::{
    BackrunConfig, BlockStateChangeProcessorActor, PendingTxStateChangeProcessorActor, StateChangeArbSearcherActor,
};
use kabu_strategy_merger::{ArbSwapPathMergerActor, DiffPathMergerActor, SamePathMergerActor};
use kabu_types_blockchain::{KabuDataTypes, KabuDataTypesEthereum};
use kabu_types_entities::{BlockHistoryState, TxSigners};
use kabu_types_market::{PoolClass, RequiredState};
use kabu_types_swap::SwapEncoder;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct BlockchainActors<
    P,
    N,
    DB: Clone + Send + Sync + 'static,
    E: Clone = MulticallerSwapEncoder,
    LDT: KabuDataTypes + 'static = KabuDataTypesEthereum,
> {
    provider: P,
    bc: Blockchain<LDT>,
    state: BlockchainState<DB, LDT>,
    strategy: Strategy<DB, LDT>,
    pub signers: SharedState<TxSigners<LDT>>,
    actor_manager: ActorsManager,
    encoder: Option<E>,
    has_mempool: bool,
    has_state_update: bool,
    has_signers: bool,
    mutlicaller_address: Option<Address>,
    relays: Vec<RelayConfig>,
    _n: PhantomData<N>,
}

impl<P, DB, E> BlockchainActors<P, Ethereum, DB, E, KabuDataTypesEthereum>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<KabuDataTypesEthereum>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    E: SwapEncoder + Send + Sync + Clone + 'static,
{
    pub fn new(
        provider: P,
        encoder: E,
        bc: Blockchain<KabuDataTypesEthereum>,
        state: BlockchainState<DB, KabuDataTypesEthereum>,
        strategy: Strategy<DB, KabuDataTypesEthereum>,
        relays: Vec<RelayConfig>,
    ) -> Self {
        Self {
            provider,
            bc,
            state,
            strategy,
            signers: SharedState::new(TxSigners::new()),
            actor_manager: ActorsManager::new(),
            encoder: Some(encoder),
            has_mempool: false,
            has_state_update: false,
            has_signers: false,
            mutlicaller_address: None,
            relays,
            _n: PhantomData,
        }
    }

    pub async fn wait(self) {
        self.actor_manager.wait().await
    }

    /// Start a custom actor
    pub fn start(&mut self, actor: impl Actor + 'static) -> Result<&mut Self> {
        self.actor_manager.start(actor)?;
        Ok(self)
    }

    /// Start a custom actor and wait for it to finish
    pub fn start_and_wait(&mut self, actor: impl Actor + Send + Sync + 'static) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(actor)?;
        Ok(self)
    }

    /// Initialize signers with the default anvil Private Key
    pub fn initialize_signers_with_anvil(&mut self) -> Result<&mut Self> {
        let key: B256 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".parse()?;

        self.actor_manager.start_and_wait(
            InitializeSignersOneShotBlockingActor::new(Some(key.to_vec())).with_signers(self.signers.clone()).on_bc(&self.bc),
        )?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with the private key. Random key generated if param in None
    pub fn initialize_signers_with_key(&mut self, key: Option<Vec<u8>>) -> Result<&mut Self> {
        self.actor_manager
            .start_and_wait(InitializeSignersOneShotBlockingActor::new(key).with_signers(self.signers.clone()).on_bc(&self.bc))?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with multiple private keys
    pub fn initialize_signers_with_keys(&mut self, keys: Vec<Vec<u8>>) -> Result<&mut Self> {
        for key in keys {
            self.actor_manager
                .start_and_wait(InitializeSignersOneShotBlockingActor::new(Some(key)).with_signers(self.signers.clone()).on_bc(&self.bc))?;
        }
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with encrypted private key
    pub fn initialize_signers_with_encrypted_key(&mut self, key: Vec<u8>) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(
            InitializeSignersOneShotBlockingActor::new_from_encrypted_key(key).with_signers(self.signers.clone()).on_bc(&self.bc),
        )?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initializes signers with encrypted key form DATA env var
    pub fn initialize_signers_with_env(&mut self) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(
            InitializeSignersOneShotBlockingActor::new_from_encrypted_env().with_signers(self.signers.clone()).on_bc(&self.bc),
        )?;
        self.with_signers()?;
        Ok(self)
    }

    /// Starts signer actor
    pub fn with_signers(&mut self) -> Result<&mut Self> {
        if !self.has_signers {
            self.has_signers = true;
            self.actor_manager.start(TxSignersActor::new().on_bc(&self.bc))?;
        }
        Ok(self)
    }

    /// Initializes encoder and start encoder actor
    pub fn with_swap_encoder(&mut self, swap_encoder: E) -> Result<&mut Self> {
        self.mutlicaller_address = Some(swap_encoder.address());
        self.encoder = Some(swap_encoder);
        self.actor_manager.start(
            SwapRouterActor::<DB, KabuDataTypesEthereum>::new().with_signers(self.signers.clone()).on_bc(&self.bc, &self.strategy),
        )?;
        Ok(self)
    }

    /// Starts market state preloader
    pub fn with_market_state_preloader(&mut self) -> Result<&mut Self> {
        let mut address_vec = self.signers.inner().try_read()?.get_address_vec();

        if let Some(kabu_multicaller) = self.mutlicaller_address {
            address_vec.push(kabu_multicaller);
        }

        self.actor_manager.start_and_wait(
            MarketStatePreloadedOneShotActor::new(self.provider.clone()).with_copied_accounts(address_vec).on_bc(&self.bc, &self.state),
        )?;
        Ok(self)
    }

    /// Starts preloaded virtual artefacts
    pub fn with_market_state_preloader_virtual(&mut self, address_to_copy: Vec<Address>) -> Result<&mut Self> {
        let address_vec = self.signers.inner().try_read()?.get_address_vec();

        let mut market_state_preloader = MarketStatePreloadedOneShotActor::new(self.provider.clone());

        for address in address_vec {
            //            market_state_preloader = market_state_preloader.with_new_account(address, 0, NWETH::from_float(10.0), None);
            market_state_preloader = market_state_preloader.with_copied_account(address).with_token_balance(
                TokenAddressEth::ETH_NATIVE,
                address,
                NWETH::from_float(10.0),
            );
        }

        market_state_preloader = market_state_preloader.with_copied_accounts(address_to_copy);

        market_state_preloader = market_state_preloader.with_new_account(
            kabu_execution_multicaller::DEFAULT_VIRTUAL_ADDRESS,
            0,
            U256::ZERO,
            kabu_execution_multicaller::MulticallerDeployer::new().account_info().code,
        );

        market_state_preloader = market_state_preloader.with_token_balance(
            TokenAddressEth::WETH,
            kabu_execution_multicaller::DEFAULT_VIRTUAL_ADDRESS,
            NWETH::from_float(10.0),
        );

        self.mutlicaller_address = Some(kabu_execution_multicaller::DEFAULT_VIRTUAL_ADDRESS);

        self.actor_manager.start_and_wait(market_state_preloader.on_bc(&self.bc, &self.state))?;
        Ok(self)
    }

    /// Starts nonce and balance monitor
    pub fn with_nonce_and_balance_monitor(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(NonceAndBalanceMonitorActor::new(self.provider.clone()).on_bc(&self.bc))?;
        Ok(self)
    }

    pub fn with_nonce_and_balance_monitor_only_once(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(NonceAndBalanceMonitorActor::new(self.provider.clone()).only_once().on_bc(&self.bc))?;
        Ok(self)
    }

    /// Starts block history actor
    pub fn with_block_history(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(BlockHistoryActor::new(self.provider.clone()).on_bc(&self.bc, &self.state))?;
        Ok(self)
    }

    /// Starts token price calculator
    pub fn with_price_station(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(PriceActor::new(self.provider.clone()).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Starts receiving blocks events through RPC
    pub fn with_block_events(&mut self, config: NodeBlockActorConfig) -> Result<&mut Self> {
        self.actor_manager.start(NodeBlockActor::new(self.provider.clone(), config).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Starts mempool actor collecting pending txes from all mempools and pulling new tx hashes in mempool_events channel
    pub fn mempool(&mut self) -> Result<&mut Self> {
        if !self.has_mempool {
            self.has_mempool = true;
            self.actor_manager.start(MempoolActor::new().on_bc(&self.bc))?;
        }
        Ok(self)
    }

    /// Starts local node pending tx provider
    pub fn with_local_mempool_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        self.actor_manager.start(NodeMempoolActor::new(self.provider.clone()).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Starts remote node pending tx provider
    pub fn with_remote_mempool<PM>(&mut self, provider: PM) -> Result<&mut Self>
    where
        PM: Provider<Ethereum> + Send + Sync + Clone + 'static,
    {
        self.mempool()?;
        self.actor_manager.start(NodeMempoolActor::new(provider).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Starts flashbots broadcaster
    pub fn with_flashbots_broadcaster(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        let flashbots_actor = match self.relays.is_empty() {
            true => FlashbotsBroadcastActor::new(None, allow_broadcast)?.with_default_relays()?,
            false => FlashbotsBroadcastActor::new(None, allow_broadcast)?.with_relays(self.relays.clone())?,
        };

        self.actor_manager.start(flashbots_actor.on_bc(&self.bc))?;
        Ok(self)
    }

    /// Start composer : estimator, signer and broadcaster
    pub fn with_composers(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        self.with_evm_estimator()?.with_signers()?.with_flashbots_broadcaster(allow_broadcast)
    }

    /// Starts pool health monitor
    pub fn with_health_monitor_pools(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(PoolHealthMonitorActor::new().on_bc(&self.bc))?;
        Ok(self)
    }

    //TODO : Move out of Blockchain
    /*
    /// Starts state health monitor
    pub fn with_health_monitor_state(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(StateHealthMonitorActor::new(self.provider.clone()).on_bc(&self.bc))?;
        Ok(self)
    }

     */

    /// Starts stuffing tx monitor
    pub fn with_health_monitor_stuffing_tx(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(StuffingTxMonitorActor::new(self.provider.clone()).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Start pool loader from new block events
    pub fn with_new_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loader =
            Arc::new(PoolLoadersBuilder::<P, Ethereum, KabuDataTypesEthereum>::default_pool_loaders(self.provider.clone(), pools_config));
        self.actor_manager.start(NewPoolLoaderActor::new(pool_loader).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Start pool loader for last 10000 blocks
    pub fn with_pool_history_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders =
            Arc::new(PoolLoadersBuilder::<P, Ethereum, KabuDataTypesEthereum>::default_pool_loaders(self.provider.clone(), pools_config));
        self.actor_manager.start(HistoryPoolLoaderOneShotActor::new(self.provider.clone(), pool_loaders).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Start pool loader from new block events
    pub fn with_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::<P, Ethereum, KabuDataTypesEthereum>::default_pool_loaders(
            self.provider.clone(),
            pools_config.clone(),
        ));
        self.actor_manager.start(PoolLoaderActor::new(self.provider.clone(), pool_loaders, pools_config).on_bc(&self.bc, &self.state))?;
        Ok(self)
    }

    /// Start pool loader for curve + steth + wsteth
    pub fn with_curve_pool_protocol_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders =
            Arc::new(PoolLoadersBuilder::<P, Ethereum, KabuDataTypesEthereum>::default_pool_loaders(self.provider.clone(), pools_config));
        self.actor_manager.start(ProtocolPoolLoaderOneShotActor::new(self.provider.clone(), pool_loaders).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Start all pool loaders
    pub fn with_pool_loaders(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        if pools_config.is_enabled(PoolClass::Curve) {
            self.with_new_pool_loader(pools_config.clone())?
                .with_pool_history_loader(pools_config.clone())?
                .with_curve_pool_protocol_loader(pools_config.clone())?
                .with_pool_loader(pools_config)
        } else {
            self.with_new_pool_loader(pools_config.clone())?.with_pool_history_loader(pools_config.clone())?.with_pool_loader(pools_config)
        }
    }

    //
    pub fn with_preloaded_state(&mut self, pools: Vec<(Address, PoolClass)>, state_required: Option<RequiredState>) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::<P, Ethereum, KabuDataTypesEthereum>::default_pool_loaders(
            self.provider.clone(),
            PoolsLoadingConfig::default(),
        ));
        let mut actor = RequiredPoolLoaderActor::new(self.provider.clone(), pool_loaders);

        for (pool_address, pool_class) in pools {
            actor = actor.with_pool_address(pool_address, pool_class);
        }

        if let Some(state_required) = state_required {
            actor = actor.with_required_state(state_required);
        }

        self.actor_manager.start_and_wait(actor.on_bc(&self.bc, &self.state))?;
        Ok(self)
    }

    /// Starts EVM gas estimator and tips filler
    pub fn with_evm_estimator(&mut self) -> Result<&mut Self> {
        self.actor_manager
            .start(EvmEstimatorActor::<P, Ethereum, E, DB>::new(self.encoder.clone().unwrap()).on_bc(&self.bc, &self.strategy))?;
        Ok(self)
    }

    /// Starts EVM gas estimator and tips filler
    pub fn with_evm_estimator_and_provider(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(
            EvmEstimatorActor::new_with_provider(self.encoder.clone().unwrap(), Some(self.provider.clone()))
                .on_bc(&self.bc, &self.strategy),
        )?;
        Ok(self)
    }

    /// Start swap path merger
    pub fn with_swap_path_merger(&mut self) -> Result<&mut Self> {
        let mutlicaller_address = self.encoder.clone().ok_or(eyre!("NO_ENCODER"))?.address();

        self.actor_manager.start(ArbSwapPathMergerActor::new(mutlicaller_address).on_bc(&self.bc, &self.strategy))?;
        Ok(self)
    }

    /// Start same path merger
    pub fn with_same_path_merger(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(SamePathMergerActor::new(self.provider.clone()).on_bc(&self.bc, &self.state, &self.strategy))?;
        Ok(self)
    }

    /// Start diff path merger
    pub fn with_diff_path_merger(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(DiffPathMergerActor::<DB>::new().on_bc(&self.bc))?;
        Ok(self)
    }

    /// Start all mergers
    pub fn with_mergers(&mut self) -> Result<&mut Self> {
        self.with_swap_path_merger()?.with_same_path_merger()?.with_diff_path_merger()
    }

    /// Start backrun on block
    pub fn with_backrun_block(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        if !self.has_state_update {
            self.actor_manager.start(StateChangeArbSearcherActor::new(backrun_config).on_bc(&self.bc, &self.strategy))?;
            self.has_state_update = true
        }
        self.actor_manager.start(BlockStateChangeProcessorActor::new().on_bc(&self.bc, &self.state, &self.strategy))?;
        Ok(self)
    }

    /// Start backrun for pending txs
    pub fn with_backrun_mempool(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        if !self.has_state_update {
            self.actor_manager.start(StateChangeArbSearcherActor::new(backrun_config).on_bc(&self.bc, &self.strategy))?;
            self.has_state_update = true
        }
        self.actor_manager.start(PendingTxStateChangeProcessorActor::new(self.provider.clone()).on_bc(
            &self.bc,
            &self.state,
            &self.strategy,
        ))?;
        Ok(self)
    }

    /// Start backrun for blocks and pending txs
    pub async fn with_backrun(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        self.with_backrun_block(backrun_config.clone())?.with_backrun_mempool(backrun_config)
    }

    /// Start influxdb writer
    pub fn with_influxdb_writer(&mut self, url: String, database: String, tags: HashMap<String, String>) -> Result<&mut Self> {
        self.actor_manager.start(InfluxDbWriterActor::new(url, database, tags).on_bc(&self.bc))?;
        Ok(self)
    }

    /// Start block latency recorder
    pub fn with_block_latency_recorder(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(MetricsRecorderActor::new().on_bc(&self.bc, &self.state))?;
        Ok(self)
    }

    /// Start web server
    pub fn with_web_server<S>(&mut self, host: String, router: Router<S>, db_pool: DbPool) -> Result<&mut Self>
    where
        S: Clone + Send + Sync + 'static,
        Router: From<Router<S>>,
    {
        self.actor_manager
            .start(WebServerActor::<S, DB>::new(host, router, db_pool, CancellationToken::new()).on_bc(&self.bc, &self.state))?;
        Ok(self)
    }

    /// Wait for node sync
    pub fn with_wait_for_node_sync(&mut self) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(WaitForNodeSyncOneShotBlockingActor::new(self.provider.clone()))?;
        Ok(self)
    }
}
