use crate::{BuilderContext, CompositeComponent};
use eyre::Result;
use tracing::info;

/// MEV-specific component builders
/// Pool loader builder - can create multiple pool loaders
#[derive(Clone)]
pub struct PoolLoaderBuilder {
    pool_types: Vec<PoolType>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum PoolType {
    UniswapV2,
    UniswapV3,
    Curve,
    Balancer,
    Custom(String),
}

impl Default for PoolLoaderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolLoaderBuilder {
    pub fn new() -> Self {
        Self { pool_types: Vec::new() }
    }

    pub fn with_uniswap_v2(mut self) -> Self {
        self.pool_types.push(PoolType::UniswapV2);
        self
    }

    pub fn with_uniswap_v3(mut self) -> Self {
        self.pool_types.push(PoolType::UniswapV3);
        self
    }

    pub fn with_curve(mut self) -> Self {
        self.pool_types.push(PoolType::Curve);
        self
    }

    pub fn with_all_default(self) -> Self {
        self.with_uniswap_v2().with_uniswap_v3().with_curve()
    }
}

/// Price monitor builder
#[derive(Clone, Default)]
pub struct PriceMonitorBuilder {
    update_interval_ms: Option<u64>,
}

impl PriceMonitorBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_update_interval(mut self, ms: u64) -> Self {
        self.update_interval_ms = Some(ms);
        self
    }
}

/// Block monitor builder
#[derive(Clone, Default)]
pub struct BlockMonitorBuilder {
    process_logs: bool,
    process_state_changes: bool,
}

impl BlockMonitorBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_logs(mut self) -> Self {
        self.process_logs = true;
        self
    }

    pub fn with_state_changes(mut self) -> Self {
        self.process_state_changes = true;
        self
    }

    pub fn all_enabled(self) -> Self {
        self.with_logs().with_state_changes()
    }
}

/// Strategy runner builder - handles multiple strategies
#[derive(Clone)]
pub struct StrategyRunnerBuilder {
    strategies: Vec<StrategyConfig>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub enum StrategyConfig {
    Backrun { max_gas_price: u64 },
    Sandwich { profit_threshold: u64 },
    Liquidation { health_threshold: u64 },
}

impl Default for StrategyRunnerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl StrategyRunnerBuilder {
    pub fn new() -> Self {
        Self { strategies: Vec::new() }
    }

    pub fn with_backrun(mut self, max_gas_price: u64) -> Self {
        self.strategies.push(StrategyConfig::Backrun { max_gas_price });
        self
    }

    pub fn with_sandwich(mut self, profit_threshold: u64) -> Self {
        self.strategies.push(StrategyConfig::Sandwich { profit_threshold });
        self
    }
}

/// Transaction signer builder
#[derive(Clone)]
pub struct SignerBuilder {
    keys: Vec<Vec<u8>>,
    use_flashbots: bool,
    gas_price_buffer: u64,
}

impl Default for SignerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SignerBuilder {
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            use_flashbots: false,
            gas_price_buffer: 10, // Default 10% buffer
        }
    }

    pub fn with_key(mut self, key: Vec<u8>) -> Self {
        self.keys.push(key);
        self
    }

    pub fn with_flashbots(mut self) -> Self {
        self.use_flashbots = true;
        self
    }

    pub fn with_gas_price_buffer(mut self, buffer_percent: u64) -> Self {
        self.gas_price_buffer = buffer_percent;
        self
    }
}

/// Health monitor builder
#[derive(Clone, Default)]
pub struct HealthMonitorBuilder {
    monitor_pools: bool,
    monitor_state: bool,
    monitor_mempool: bool,
}

impl HealthMonitorBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_pool_monitoring(mut self) -> Self {
        self.monitor_pools = true;
        self
    }

    pub fn with_state_monitoring(mut self) -> Self {
        self.monitor_state = true;
        self
    }

    pub fn with_mempool_monitoring(mut self) -> Self {
        self.monitor_mempool = true;
        self
    }
}

/// Account monitor builder
#[derive(Clone, Default)]
pub struct AccountMonitorBuilder {
    update_interval_secs: u64,
}

impl AccountMonitorBuilder {
    pub fn new() -> Self {
        Self { update_interval_secs: 30 }
    }

    pub fn with_update_interval(mut self, interval_secs: u64) -> Self {
        self.update_interval_secs = interval_secs;
        self
    }
}

/// Complete MEV bot components builder
pub struct MevBotComponentsBuilder {
    pool_loaders: Option<PoolLoaderBuilder>,
    price_monitor: Option<PriceMonitorBuilder>,
    block_monitor: Option<BlockMonitorBuilder>,
    strategy_runner: Option<StrategyRunnerBuilder>,
    signer: Option<SignerBuilder>,
    health_monitor: Option<HealthMonitorBuilder>,
    account_monitor: Option<AccountMonitorBuilder>,
}

impl Default for MevBotComponentsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MevBotComponentsBuilder {
    pub fn new() -> Self {
        Self {
            pool_loaders: None,
            price_monitor: None,
            block_monitor: None,
            strategy_runner: None,
            signer: None,
            health_monitor: None,
            account_monitor: None,
        }
    }

    pub fn pool_loaders(mut self, builder: PoolLoaderBuilder) -> Self {
        self.pool_loaders = Some(builder);
        self
    }

    pub fn price_monitor(mut self, builder: PriceMonitorBuilder) -> Self {
        self.price_monitor = Some(builder);
        self
    }

    pub fn block_monitor(mut self, builder: BlockMonitorBuilder) -> Self {
        self.block_monitor = Some(builder);
        self
    }

    pub fn strategy_runner(mut self, builder: StrategyRunnerBuilder) -> Self {
        self.strategy_runner = Some(builder);
        self
    }

    pub fn signer(mut self, builder: SignerBuilder) -> Self {
        self.signer = Some(builder);
        self
    }

    pub fn health_monitor(mut self, builder: HealthMonitorBuilder) -> Self {
        self.health_monitor = Some(builder);
        self
    }

    pub fn account_monitor(mut self, builder: AccountMonitorBuilder) -> Self {
        self.account_monitor = Some(builder);
        self
    }

    /// Build a composite component containing all MEV bot components
    pub async fn build<State>(self, _ctx: &BuilderContext<State>) -> Result<CompositeComponent> {
        let composite = CompositeComponent::new("MevBot");

        // Add components in dependency order

        // 1. Pool loaders (no dependencies)
        if let Some(pool_loader_builder) = self.pool_loaders {
            info!("Building pool loader components for types: {:?}", pool_loader_builder.pool_types);
            // Pool loaders are now implemented as HistoryPoolLoaderComponent and ProtocolPoolLoaderComponent
            // These would be instantiated with the specific pool types from pool_loader_builder
        }

        // 2. Price monitor (depends on pools)
        if let Some(price_monitor) = self.price_monitor {
            info!("Building price monitor component with update interval: {:?}ms", price_monitor.update_interval_ms);
            // Price monitoring is handled by existing price actors that are already component-compatible
        }

        // 3. Block monitor (no dependencies)
        if let Some(block_monitor) = self.block_monitor {
            info!(
                "Building block monitor component with settings: logs={}, state_changes={}",
                block_monitor.process_logs, block_monitor.process_state_changes
            );
            // Block monitoring is now handled by BlockProcessingComponent
        }

        // 4. Strategy runner (depends on pools, prices)
        if let Some(strategy_runner) = self.strategy_runner {
            info!("Building strategy runner component with {} strategies", strategy_runner.strategies.len());
            // Strategy execution is handled by existing strategy components (StateChangeArbSearcherActor, etc.)
        }

        // 5. Signer (depends on strategies)
        if let Some(signer) = self.signer {
            info!(
                "Building signer component with {} keys, flashbots={}, gas_buffer={}%",
                signer.keys.len(),
                signer.use_flashbots,
                signer.gas_price_buffer
            );
            // Transaction signing is handled by existing SignersComponent
            // Configuration includes private keys, flashbots relay settings, and gas price buffers
        }

        // 6. Health monitors (observes all)
        if let Some(health_monitor) = self.health_monitor {
            info!(
                "Building health monitor component with settings: pools={}, state={}, mempool={}",
                health_monitor.monitor_pools, health_monitor.monitor_state, health_monitor.monitor_mempool
            );
            // Health monitoring is handled by existing StateHealthMonitorActor and related components
            // Monitors pool health, state consistency, and mempool activity
        }

        Ok(composite)
    }
}
