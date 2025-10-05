# Architecture

Kabu is built on a modern component-based architecture that emphasizes modularity, concurrency, and type safety. The system efficiently processes blockchain data and identifies arbitrage opportunities through a unified node design with pluggable components.

## Core Design Principles

1. **Node-Based Architecture**: Centralized node pattern with pluggable components
2. **Component-Based Concurrency**: Each component runs independently with message-passing communication
3. **Type Safety**: Extensive use of Rust's type system for correctness
4. **Modular Design**: Clear separation between data types, business logic, and infrastructure
5. **Performance First**: Optimized for low-latency arbitrage detection and execution

## Node System

The new node system provides a unified way to build and launch Kabu instances:

### KabuNode Trait

The `KabuNode` trait defines the interface for different node implementations:

```rust,ignore
pub trait KabuNode<P, DB>: Send + Sync + 'static {
    async fn build_components(
        &self,
        context: &KabuBuildContext<P, DB>,
    ) -> Result<Vec<BoxedComponent>>;
}
```

### KabuBuilder

The `KabuBuilder` provides a fluent API for constructing and launching nodes:

```rust,ignore
let handle = KabuBuilder::new(kabu_context)
    .node(KabuEthereumNode::<P, DB>::default())
    .build()
    .launch(task_executor)
    .await?;
```

### KabuBuildContext

The build context centralizes all configuration and shared resources:

```rust,ignore
let kabu_context = KabuBuildContextBuilder::new(
    provider,
    blockchain,
    blockchain_state,
    topology_config,
    backrun_config,
    multicaller_address,
    db_pool,
    is_exex,
)
.with_pools_config(pools_config)
.with_swap_encoder(swap_encoder)
.with_channels(mev_channels)
.build();
```

## Component System

Components are independent processing units that:

- Implement the `Component` trait
- Run as tokio tasks via `TaskExecutor`
- Communicate through `MevComponentChannels`
- Use builder pattern for configuration
- Support graceful shutdown

### Key Components

- **StateChangeArbSearcherComponent**: Detects arbitrage in state changes
- **SwapRouterComponent**: Routes and encodes swap transactions  
- **SignersComponent**: Manages transaction signing with nonce tracking
- **FlashbotsBroadcastComponent**: Submits bundles to Flashbots
- **Market Components**: Pool discovery and state management
- **PoolHealthMonitorComponent**: Tracks pool reliability

## Type System Organization

Kabu's type system is split into three crates for modularity:

### types/entities
Core blockchain and configuration types:
- `Block`, `Transaction`, `Account`
- `StrategyConfig` trait and implementations
- Pool loading configurations

### types/market  
Market structure and routing:
- `Token`: ERC20 with decimals, price, categories
- `Pool` trait: Unified pool interface
- `Market`: Registry of tokens and pools
- `SwapDirection`: Token pair direction in pool
- `SwapPath`: Route through multiple pools

### types/swap
Execution and profit tracking:
- `Swap`: Final transaction ready for execution
- `SwapLine`: Path with amounts and gas costs
- `SwapStep`: Single pool interaction
- Calculation utilities for profit estimation

## Communication Architecture

### MevComponentChannels

All component communication is centralized through `MevComponentChannels`:

```rust,ignore
pub struct MevComponentChannels<DB> {
    // Market events
    pub market_events: broadcast::Sender<MarketEvents>,
    pub mempool_events: broadcast::Sender<MempoolEvents>,
    
    // Swap processing
    pub swap_compose: broadcast::Sender<SwapComposeMessage>,
    pub estimated_swaps: broadcast::Sender<SwapComposeMessage>,
    
    // Health monitoring
    pub health_events: broadcast::Sender<HealthEvent>,
    
    // Shared state
    pub signers: Arc<RwLock<TxSigners>>,
    pub account_state: Arc<RwLock<AccountNonceAndBalanceState>>,
}
```

### Message Flow

The typical arbitrage detection and execution flow:

```text
Block/Mempool Event
    ↓
StateChangeArbSearcher
    ↓
SwapCompose (prepare)
    ↓
Merger Components
    ↓
EvmEstimator
    ↓
SwapRouter
    ↓
SignersComponent
    ↓
FlashbotsBroadcast
```

## Building a Kabu Instance

### Using KabuEthereumNode

The standard way to create a Kabu instance:

```rust,ignore
// Create provider
let provider = ProviderBuilder::new()
    .on_http(node_url)
    .build()?;

// Initialize blockchain and state
let blockchain = Blockchain::new(chain_id);
let blockchain_state = BlockchainState::<KabuDB, KabuDataTypesEthereum>::new();

// Build context
let kabu_context = KabuBuildContextBuilder::new(
    provider,
    blockchain,
    blockchain_state,
    topology_config,
    backrun_config,
    multicaller_address,
    db_pool,
    false, // is_exex
)
.build();

// Launch with KabuBuilder
let handle = KabuBuilder::new(kabu_context)
    .node(KabuEthereumNode::default())
    .build()
    .launch(task_executor)
    .await?;

// Wait for shutdown
handle.wait_for_shutdown().await?;
```

### Custom Node Implementation

You can create custom nodes by implementing the `KabuNode` trait:

```rust,ignore
pub struct CustomNode;

impl<P, DB> KabuNode<P, DB> for CustomNode 
where
    P: Provider + Send + Sync + 'static,
    DB: Database + Send + Sync + 'static,
{
    async fn build_components(
        &self,
        context: &KabuBuildContext<P, DB>,
    ) -> Result<Vec<BoxedComponent>> {
        let mut components = vec![];
        
        // Add your custom components
        components.push(Box::new(
            MyCustomComponent::new(context.config.clone())
                .with_channels(&context.channels)
        ));
        
        Ok(components)
    }
}
```

This architecture provides:
- Unified node construction
- Centralized configuration
- Easy component composition
- Clean shutdown handling
- Flexible customization