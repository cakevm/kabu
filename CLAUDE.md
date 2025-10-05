# Kabu MEV Bot - Claude Development Guide

This guide helps Claude instances understand and work with the Kabu MEV bot codebase effectively.

## Quick Start Commands

```bash
# Run tests
make test

# Pre-release checks (includes fmt, clippy, taplo, and udeps)
make pre-release

# Clean unused dependencies
make udeps

# Type checking and linting
make lint
make check
make clippy  # Run clippy on all targets including tests and benches

# Build the project
cargo build --release

# Run the main binary
cargo run -p kabu

# Run specific swap tests
make swap-test FILE=./testing/backtest-runner/test_18567709.toml
make swap-test-all  # Run all swap tests
```

## Architecture Overview

Kabu is an MEV (Maximum Extractable Value) bot built with Rust, using a component-based architecture (evolved from actor-based) for concurrent processing of blockchain data and arbitrage opportunities.

### Core Design Principles

1. **Component-Based Concurrency**: Each component runs independently with message-passing communication
2. **Type Safety**: Extensive use of Rust's type system for correctness
3. **Modular Design**: Clear separation between data types, business logic, and infrastructure
4. **Performance First**: Optimized for low-latency arbitrage detection and execution

### Key Components

1. **Component System** (`crates/core/components/`)
   - Message-passing concurrency model using tokio
   - Components communicate via broadcast channels
   - Builder pattern for component initialization
   - Key components:
     - StateChangeArbSearcherComponent: Finds arbitrage in state changes
     - SwapRouterComponent: Routes and encodes swap transactions
     - TxSignersComponent: Signs transactions with managed keys
     - Broadcast components: Submit to network/flashbots

2. **Type System** (split into three crates for modularity)
   - `crates/types/entities/` - Core blockchain entities
     - Block, Transaction, Account structures
     - Strategy configurations
     - Pool configurations
   - `crates/types/market/` - Market-related types
     - Token: ERC20 token with price data
     - Pool trait and implementations
     - Market: Pool and token registry
     - SwapDirection, SwapPath, SwapPathBuilder
   - `crates/types/swap/` - Swap execution types
     - Swap: Executable arbitrage transaction
     - SwapLine: Path with calculated amounts
     - SwapStep: Individual pool interaction

3. **Strategy Layer** (`crates/strategy/`)
   - `backrun/`: State change arbitrage detection
     - BackrunConfig: Strategy configuration
     - StateChangeArbSearcher: Main arbitrage finder
     - SwapCalculator: Profit calculation logic
   - `merger/`: Transaction optimization
     - SamePathMerger: Combines similar paths
     - DiffPathMerger: Merges different opportunities
     - ArbSwapPathMerger: Multi-path arbitrage

4. **Execution Layer** (`crates/execution/`)
   - `estimator/`: EVM simulation for gas and success estimation
   - `multicaller/`: Custom contract for batch operations
     - Deploy logic for multicaller contract
     - Encoding for complex swap sequences

5. **DeFi Integrations** (`crates/defi/`)
   - `pools/`: Protocol implementations
     - UniswapV2Pool, UniswapV3Pool
     - CurvePool with multiple implementations
     - MaverickPool, PancakeV3Pool
   - `market/`: Market state management
   - `preloader/`: Initial state loading
   - `health_monitor/`: Pool reliability tracking

6. **Node Interaction** (`crates/node/`)
   - WebSocket subscription management
   - Block and transaction streaming
   - State diff calculation

7. **Database Layer** (`crates/evm/db/`)
   - In-memory state cache
   - Fork database for simulations
   - State diff application

## Component Communication Pattern

```rust
// Typical component setup using builder pattern
let component = StateChangeArbSearcherComponent::new(backrun_config)
    .on_bc(&blockchain, &strategy)  // Wire blockchain channels
    .with_market(market.clone());    // Set shared state

component.spawn(executor)?;          // Spawn with task executor

// Or using the centralized builder
let kabu_context = KabuBuildContextBuilder::new(
    provider.clone(),
    blockchain,
    blockchain_state.clone(),
    topology_config.clone(),
    backrun_config.clone(),
    multicaller_address,
    db_pool.clone(),
    is_exex,
)
.with_pools_config(pools_config.clone())
.with_swap_encoder(swap_encoder.clone())
.build();

let components = KabuComponentsBuilder::new(&kabu_context)
    .with_market_components()
    .with_network_components()
    .with_monitoring_components()
    .build()
    .await?;
```

## Type System Organization

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
- `PoolId`: Various pool identification methods

### types/swap
Execution and profit tracking:
- `Swap`: Final transaction ready for execution
- `SwapLine`: Path with amounts and gas costs
- `SwapStep`: Single pool interaction
- Calculation utilities for profit estimation

## Common Tasks

### Adding a New Pool Protocol

1. Create pool implementation:
```rust
// In crates/defi/pools/src/mynewpool.rs
#[derive(Clone, Debug)]
pub struct MyNewPool {
    address: Address,
    token0: Address,
    token1: Address,
    // ... protocol specific fields
}

impl Pool for MyNewPool {
    // Implement required methods
}
```

2. Add to pool loader:
```rust
// In crates/defi/pools/src/loaders/mod.rs
// Add detection and loading logic
```

3. Update PoolClass enum if needed:
```rust
// In crates/types/market/src/pool_class.rs
pub enum PoolClass {
    // ... existing variants
    MyNewProtocol,
}
```

### Creating a New Component

1. Define component struct:
```rust
pub struct MyComponent<DB: Clone + Send + Sync + 'static> {
    config: MyConfig,
    market: Option<Arc<RwLock<Market>>>,
    input_rx: Option<broadcast::Sender<InputMessage>>,
    output_tx: Option<broadcast::Sender<OutputMessage>>,
}
```

2. Implement builder methods:
```rust
impl<DB> MyComponent<DB> {
    pub fn new(config: MyConfig) -> Self {
        Self {
            config,
            market: None,
            input_rx: None,
            output_tx: None,
        }
    }
    
    pub fn with_market(self, market: Arc<RwLock<Market>>) -> Self {
        Self { market: Some(market), ..self }
    }
    
    pub fn on_bc(self, bc: &Blockchain) -> Self {
        Self {
            market: Some(bc.market()),
            input_rx: Some(bc.some_channel()),
            ..self
        }
    }
}
```

3. Implement Component trait:
```rust
impl<DB> Component for MyComponent<DB> 
where
    DB: DatabaseRef + Send + Sync + Clone + 'static,
{
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();
        
        let market = self.market.ok_or_else(|| eyre!("market not set"))?;
        let input_rx = self.input_rx.ok_or_else(|| eyre!("input_rx not set"))?.subscribe();
        
        executor.spawn_critical(name, async move {
            if let Err(e) = my_component_worker(market, input_rx).await {
                error!("Component failed: {}", e);
            }
        });
        
        Ok(())
    }
    
    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }
    
    fn name(&self) -> &'static str {
        "MyComponent"
    }
}
```

### Working with Arbitrage Detection

1. **State Change Detection**: 
   - Monitor `StateUpdateEvent` for pool changes
   - Build affected swap paths
   - Calculate profitability with SwapCalculator

2. **Swap Optimization**:
   - Use merger actors to combine opportunities
   - Estimate gas with EvmEstimator
   - Encode with multicaller for efficiency

3. **Execution Flow**:
   ```
   StateChange → ArbSearcher → SwapCompose → Estimator → 
   Router → Signer → Broadcaster
   ```

## Testing Infrastructure

### Unit Tests
- Located alongside source files
- Mock implementations in test modules
- Run: `cargo test -p <package-name>`

### Integration Testing (`testing/`)

**backtest-runner**: Historical simulation
- Replays blocks with state
- Validates arbitrage detection
- Measures performance metrics

**gasbench**: Gas optimization
- Profiles contract interactions
- Compares encoding strategies

**nodebench**: Infrastructure testing
- Measures node latency
- Tests concurrent connections

**replayer**: Transaction analysis
- Replays specific transactions
- Debugs execution failures

## Performance Optimization

### Parallel Processing
```rust
// State change searcher uses thread pool
thread_pool.install(|| {
    swap_paths.into_par_iter().for_each_with(context, |ctx, path| {
        // Parallel calculation
    });
});
```

### Channel Sizing
- Size channels based on expected throughput
- Monitor channel depths in production
- Use bounded channels to prevent memory issues

### State Management
- Minimize lock contention with read-write locks
- Clone cheaply with Arc for immutable data
- Batch state updates when possible

## Common Patterns

### Error Handling
```rust
use eyre::{Result, WrapErr};

fn risky_operation() -> Result<Value> {
    external_call()
        .wrap_err("Failed to call external service")?;
    Ok(value)
}
```

### Message Passing
```rust
// Send with error handling
if let Err(e) = channel.send(Message::new(data)) {
    error!("Failed to send: {}", e);
}

// Receive with timeout
tokio::select! {
    msg = receiver.recv() => handle_message(msg?),
    _ = tokio::time::sleep(timeout) => handle_timeout(),
}
```

### Shared State Access
```rust
// Minimize lock duration
let data = {
    let guard = shared_state.read().await;
    guard.expensive_clone()
}; // Lock released here
process_data(data);
```

## Debugging Tips

1. **Enable Tracing**: 
   ```bash
   RUST_LOG=debug cargo run
   ```

2. **Actor Communication**: 
   - Log message sends/receives
   - Monitor channel depths
   - Track actor lifecycle

3. **Arbitrage Calculation**:
   - Log intermediate amounts
   - Verify pool states
   - Check gas estimations

4. **State Differences**:
   - Compare before/after states
   - Validate state transitions
   - Monitor pool updates

## Security Considerations

1. **Private Key Management**:
   - Never log private keys
   - Use secure key derivation
   - Rotate keys regularly

2. **Transaction Security**:
   - Validate all calculations
   - Simulate before submission
   - Monitor for reverts

3. **MEV Protection**:
   - Use flashbots when possible
   - Implement commit-reveal if needed
   - Monitor mempool exposure

## Configuration

### Strategy Configuration
Located in strategy configs, typically:
```toml
[backrun_strategy]
eoa = "0x..." # Optional execution address
smart = true  # Enable optimization
```

### Pool Loading
Configure which pools to load:
```rust
PoolsLoadingConfig {
    load_uni_v2: true,
    load_uni_v3: true,
    load_curve: true,
    // ...
}
```

## Maintenance Notes

1. **Dependency Updates**: 
   - Run `make udeps` to clean unused deps
   - Check compatibility before updating
   - Test thoroughly after updates

2. **Performance Monitoring**:
   - Profile regularly with flamegraph
   - Monitor actor performance
   - Track arbitrage success rates

3. **Code Organization**:
   - Keep actors focused and single-purpose
   - Maintain clear module boundaries
   - Document complex algorithms

## Build and Development Commands

### Essential Commands
```bash
# Build the project
make build              # Debug build
make release           # Release build  
make maxperf           # Optimized release build with native CPU optimizations

# Run tests
make test              # Run all tests (excludes some integration tests)
cargo test -p <package-name> --lib  # Run tests for specific package

# Code quality
make fmt               # Format code
make clippy            # Run linter
make pre-release       # Run all checks (fmt, clippy, taplo, udeps)
make udeps             # Check for unused dependencies

# Backtest specific swap scenarios
make swap-test FILE=./testing/backtest-runner/test_18498188.toml
make swap-test-all     # Run all swap tests

# Run replayer
make replayer
```

## Testing Tips

1. **Swap Tests**:
   - Use `make swap-test FILE=./testing/backtest-runner/test_NAME.toml`
   - Check assertions for `swaps_encoded`, `swaps_ok`, and `best_profit_eth`

2. **Debug Logging**:
   ```bash
   RUST_LOG=kabu_strategy_backrun=debug,kabu_strategy_merger=debug make swap-test
   ```

3. **Pre-release Checks**:
   - Always run `make pre-release` before committing
   - Includes formatting, clippy (with tests/benches), taplo, and unused deps
   - Fix any warnings to maintain code quality