# Examples

## Quick Start with KabuBuilder

The simplest way to start a Kabu MEV bot:

```rust,ignore
use kabu::core::blockchain::{Blockchain, BlockchainState};
use kabu::core::components::{KabuBuilder, MevComponentChannels};
use kabu::core::node::{KabuBuildContext, KabuEthereumNode};
use kabu::evm::db::KabuDB;
use kabu::execution::multicaller::MulticallerSwapEncoder;
use kabu::strategy::backrun::BackrunConfig;
use kabu::types::blockchain::KabuDataTypesEthereum;
use alloy::providers::ProviderBuilder;
use reth_tasks::TaskManager;

#[tokio::main]
async fn main() -> Result<()> {
    // Create provider
    let provider = ProviderBuilder::new()
        .on_http("http://localhost:8545")
        .build()?;
        
    // Initialize blockchain components
    let blockchain = Blockchain::new(1); // Chain ID 1 for mainnet
    let blockchain_state = BlockchainState::<KabuDB, KabuDataTypesEthereum>::new();
    
    // Load configurations
    let topology_config = TopologyConfig::load_from_file("config.toml")?;
    let backrun_config = BackrunConfig::new_dumb(); // Simple config for testing
    
    // Deploy or get multicaller address
    let multicaller_address = "0x...".parse()?;
    
    // Create task manager
    let task_manager = TaskManager::new(tokio::runtime::Handle::current());
    let task_executor = task_manager.executor();
    
    // Build and launch
    let kabu_context = KabuBuildContext::builder(
        provider,
        blockchain,
        blockchain_state,
        topology_config,
        backrun_config,
        multicaller_address,
        None, // No database for simple example
        false, // Not running as reth ExEx
    )
    .build();
    
    let handle = KabuBuilder::new(kabu_context)
        .node(KabuEthereumNode::default())
        .build()
        .launch(task_executor)
        .await?;
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    handle.shutdown().await?;
    
    Ok(())
}
```

## Running with Reth Node

Kabu can run as a Reth Execution Extension (ExEx):

```rust,ignore
use reth::cli::Cli;
use reth::builder::NodeHandle;

fn main() -> eyre::Result<()> {
    Cli::<EthereumChainSpecParser, KabuArgs>::parse().run(|builder, kabu_args| async move {
        let blockchain = Blockchain::new(builder.config().chain.chain.id());
        
        let NodeHandle { node, node_exit_future } = builder
            .with_types_and_provider::<EthereumNode, BlockchainProvider<_>>()
            .with_components(EthereumNode::components())
            .with_add_ons(EthereumAddOns::default())
            .install_exex("kabu-exex", |ctx| init_kabu_exex(ctx, blockchain))
            .launch()
            .await?;
            
        // Start Kabu MEV components
        let provider = node.provider.clone();
        let handle = start_kabu_mev(provider, blockchain, true).await?;
        
        // Wait for node exit
        node_exit_future.await?;
        Ok(())
    })
}
```

## Custom Component Example

Create a component that monitors specific pools:

```rust,ignore
use kabu_core_components::{Component, MevComponentChannels};
use kabu_types_events::MarketEvents;
use reth_tasks::TaskExecutor;

pub struct PoolMonitorComponent<DB> {
    pools_to_monitor: Vec<Address>,
    channels: Option<MevComponentChannels<DB>>,
}

impl<DB: Clone + Send + Sync + 'static> PoolMonitorComponent<DB> {
    pub fn new(pools: Vec<Address>) -> Self {
        Self {
            pools_to_monitor: pools,
            channels: None,
        }
    }
    
    pub fn with_channels(mut self, channels: &MevComponentChannels<DB>) -> Self {
        self.channels = Some(channels.clone());
        self
    }
}

impl<DB: Clone + Send + Sync + 'static> Component for PoolMonitorComponent<DB> {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let channels = self.channels.ok_or_else(|| eyre!("channels not set"))?;
        let pools = self.pools_to_monitor;
        
        let mut market_events = channels.market_events.subscribe();
        
        executor.spawn_critical("PoolMonitor", async move {
            while let Ok(event) = market_events.recv().await {
                match event {
                    MarketEvents::PoolUpdate { address, .. } => {
                        if pools.contains(&address) {
                            info!("Monitored pool updated: {:?}", address);
                            // Trigger specific actions for monitored pools
                        }
                    }
                    _ => {}
                }
            }
        });
        
        Ok(())
    }
    
    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }
    
    fn name(&self) -> &'static str {
        "PoolMonitorComponent"
    }
}
```

## Custom Node Implementation

Create a minimal node with only essential components:

```rust,ignore
use kabu_core_components::{KabuNode, KabuBuildContext, BoxedComponent};

pub struct MinimalNode;

#[async_trait]
impl<P, DB> KabuNode<P, DB> for MinimalNode 
where
    P: Provider + Send + Sync + 'static,
    DB: Database + Send + Sync + 'static,
{
    async fn build_components(
        &self,
        context: &KabuBuildContext<P, DB>,
    ) -> Result<Vec<BoxedComponent>> {
        let mut components = vec![];
        
        // Add only arbitrage searcher
        components.push(Box::new(
            StateChangeArbSearcherComponent::new(context.backrun_config.clone())
                .with_channels(&context.channels)
                .with_market_state(context.market_state.clone())
        ));
        
        // Add signer component
        components.push(Box::new(
            SignersComponent::<DB, KabuDataTypesEthereum>::new()
                .with_channels(&context.channels)
        ));
        
        // Add broadcaster
        if let Some(broadcaster_config) = context.topology_config.actors.broadcaster.as_ref() {
            components.push(Box::new(
                FlashbotsBroadcastComponent::new(
                    context.provider.clone(),
                    broadcaster_config.clone(),
                )
                .with_channels(&context.channels)
            ));
        }
        
        Ok(components)
    }
}

// Usage
let handle = KabuBuilder::new(kabu_context)
    .node(MinimalNode)
    .build()
    .launch(task_executor)
    .await?;
```

## Working with State

### Loading Pool State

When working with pools, ensure their state is loaded:

```rust,ignore
use kabu_types_market::{Pool, RequiredStateReader};
use kabu_defi_pools::UniswapV3Pool;

// Fetch pool data
let pool = UniswapV3Pool::fetch_pool_data(provider.clone(), pool_address).await?;

// Get required state
let state_required = pool.get_state_required()?;

// Fetch state from chain
let state_update = RequiredStateReader::<KabuDataTypesEthereum>::fetch_calls_and_slots(
    provider.clone(),
    state_required,
    Some(block_number),
)
.await?;

// Apply to state DB
market_state.write().await.state_db.apply_geth_update(state_update);

// Add pool to market
market.write().await.add_pool(pool.into())?;
```

### Accessing Shared State

```rust,ignore
// Read market data
let market_guard = context.market.read().await;
let pool = market_guard.get_pool(&pool_address)?;
let tokens = market_guard.tokens();
drop(market_guard); // Release lock

// Update signer state
let mut signers = context.channels.signers.write().await;
signers.add_privkey(private_key);
```

## Testing Components

```rust,no_run
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;
    
    #[tokio::test]
    async fn test_component_startup() {
        // Create test channels
        let channels = MevComponentChannels::default();
        
        // Create component
        let component = MyComponent::new(MyConfig::default())
            .with_channels(&channels);
        
        // Create test executor
        let task_manager = TaskManager::new(tokio::runtime::Handle::current());
        let executor = task_manager.executor();
        
        // Spawn component
        assert!(component.spawn(executor).is_ok());
        
        // Send test event
        channels.market_events.send(MarketEvents::BlockHeaderUpdate {
            block_number: 100,
            block_hash: Default::default(),
            timestamp: 0,
            base_fee: U256::ZERO,
            next_base_fee: U256::ZERO,
        }).unwrap();
        
        // Allow processing
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Shutdown
        task_manager.shutdown().await;
    }
}
```

## Complete Example: Custom Arbitrage Bot

See the `testing/backtest-runner` for a complete example that:
- Initializes providers and blockchain state
- Loads pools and their state
- Configures components
- Processes historical blocks
- Tracks arbitrage opportunities

Key patterns from the backtest runner:
- Uses `MevComponentChannels` for pre-initialized channels
- Loads pool state before processing
- Manually triggers events for testing
- Tracks performance metrics