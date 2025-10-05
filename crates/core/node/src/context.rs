//! Context for Kabu node configuration

use alloy_network::Ethereum;
use alloy_primitives::Address;
use alloy_provider::Provider;
use std::sync::Arc;
use tokio::sync::RwLock;

use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_core_components::MevComponentChannels;
use kabu_core_topology::TopologyConfig;
use kabu_evm_db::{DatabaseKabuExt, KabuDBError};
use kabu_execution_multicaller::MulticallerSwapEncoder;
use kabu_storage_db::DbPool;
use kabu_strategy_backrun::BackrunConfig;
use kabu_types_blockchain::{ChainParameters, Mempool};
use kabu_types_entities::{BlockHistory, BlockHistoryState};
use kabu_types_market::{Market, MarketState};
use reth::revm::{Database, DatabaseCommit, DatabaseRef};
use reth_ethereum_primitives::EthPrimitives;

/// Context that holds all shared resources for node components
#[derive(Clone)]
pub struct KabuContext<R, P, DB, Evm>
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    /// Reth provider for state access and subscriptions
    pub reth_provider: R,
    /// Provider for blockchain access
    pub provider: P,
    /// EVM configuration
    pub evm_config: Evm,
    /// Blockchain state and event channels
    pub blockchain: Blockchain,
    /// Blockchain state with database
    pub blockchain_state: BlockchainState<DB, EthPrimitives>,
    /// MEV component communication channels
    pub channels: MevComponentChannels<DB>,
    /// Shared market state (tokens and pools)
    pub market: Arc<RwLock<Market>>,
    /// Shared market state with DB
    pub market_state: Arc<RwLock<MarketState<DB>>>,
    /// Shared mempool
    pub mempool: Arc<RwLock<Mempool<EthPrimitives>>>,
    /// Shared block history
    pub block_history: Arc<RwLock<BlockHistory<DB, EthPrimitives>>>,
    /// Configuration
    pub config: NodeConfig,
}

/// Node configuration
#[derive(Clone)]
pub struct NodeConfig {
    /// Chain ID
    pub chain_id: u64,
    /// Chain parameters
    pub chain_params: ChainParameters,
    /// Topology configuration
    pub topology_config: TopologyConfig,
    /// Backrun strategy configuration
    pub backrun_config: BackrunConfig,
    /// Multicaller contract address
    pub multicaller_address: Address,
    /// Swap encoder
    pub swap_encoder: MulticallerSwapEncoder,
    /// Database pool (optional - for web server)
    pub db_pool: Option<DbPool>,
    /// Enable web server
    pub enable_web_server: bool,
    /// Pool loading configuration
    pub pools_config: kabu_defi_pools::PoolsLoadingConfig,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeConfig {
    /// Create a new node configuration with defaults
    pub fn new() -> Self {
        let multicaller_address = Address::default();
        let swap_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);

        // Default pools config
        let pools_config = kabu_defi_pools::PoolsLoadingConfig::disable_all(kabu_defi_pools::PoolsLoadingConfig::default())
            .enable(kabu_types_market::PoolClass::UniswapV2)
            .enable(kabu_types_market::PoolClass::UniswapV3);

        // Create a minimal topology config with explicit ActorConfig
        let topology_config = TopologyConfig {
            influxdb: None,
            clients: Default::default(),
            blockchains: Default::default(),
            actors: kabu_core_topology::ActorConfig {
                broadcaster: None,
                node: None,
                node_exex: None,
                mempool: None,
                price: None,
                estimator: None,
                noncebalance: None,
                pools: None,
            },
            signers: Default::default(),
            encoders: Default::default(),
            preloaders: None,
            webserver: None,
            database: None,
        };

        Self {
            chain_id: 1, // Ethereum mainnet
            chain_params: ChainParameters::ethereum(),
            topology_config,
            backrun_config: BackrunConfig::default(),
            multicaller_address,
            swap_encoder,
            db_pool: None,
            enable_web_server: false,
            pools_config,
        }
    }

    /// Set the chain ID
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// Set the chain parameters
    pub fn chain_params(mut self, chain_params: ChainParameters) -> Self {
        self.chain_params = chain_params;
        self
    }

    /// Set the topology configuration
    pub fn topology_config(mut self, topology_config: TopologyConfig) -> Self {
        self.topology_config = topology_config;
        self
    }

    /// Set the backrun configuration
    pub fn backrun_config(mut self, backrun_config: BackrunConfig) -> Self {
        self.backrun_config = backrun_config;
        self
    }

    /// Set the multicaller address
    pub fn multicaller_address(mut self, multicaller_address: Address) -> Self {
        self.multicaller_address = multicaller_address;
        self
    }

    /// Set the swap encoder
    pub fn swap_encoder(mut self, swap_encoder: MulticallerSwapEncoder) -> Self {
        self.swap_encoder = swap_encoder;
        self
    }

    /// Set the database pool
    pub fn db_pool(mut self, db_pool: DbPool) -> Self {
        self.db_pool = Some(db_pool);
        self
    }

    /// Enable web server
    pub fn enable_web_server(mut self, enable: bool) -> Self {
        self.enable_web_server = enable;
        self
    }

    /// Set the pools configuration
    pub fn pools_config(mut self, pools_config: kabu_defi_pools::PoolsLoadingConfig) -> Self {
        self.pools_config = pools_config;
        self
    }
}

impl<R, P, DB, Evm> KabuContext<R, P, DB, Evm>
where
    R: Send + Sync + Clone + 'static,
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = KabuDBError>
        + Database<Error = KabuDBError>
        + DatabaseCommit
        + DatabaseKabuExt
        + BlockHistoryState<EthPrimitives>
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    Evm: Clone + Send + Sync + 'static,
{
    /// Create a new context with the given configuration
    pub fn new(
        reth_provider: R,
        provider: P,
        evm_config: Evm,
        blockchain: Blockchain,
        blockchain_state: BlockchainState<DB, EthPrimitives>,
        config: NodeConfig,
    ) -> Self {
        // Get shared state from blockchain
        let market = blockchain.market();
        let market_state = blockchain_state.market_state_commit();
        let mempool = blockchain.mempool();
        let block_history = Arc::new(RwLock::new(BlockHistory::new(10)));

        Self {
            reth_provider,
            provider,
            evm_config,
            blockchain,
            blockchain_state,
            channels: MevComponentChannels::default(),
            market,
            market_state,
            mempool,
            block_history,
            config,
        }
    }

    /// Get the chain parameters
    pub fn chain_params(&self) -> &ChainParameters {
        &self.config.chain_params
    }
}
