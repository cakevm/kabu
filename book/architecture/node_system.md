# Node System

The Kabu node system provides a unified, extensible framework for building MEV bots with different configurations and capabilities.

## Overview

The node system consists of three main parts:
1. **KabuNode trait** - Defines how to build components
2. **KabuBuilder** - Orchestrates node construction and launch
3. **KabuBuildContext** - Centralizes configuration and resources

## KabuNode Trait

The `KabuNode` trait is the core abstraction for creating different node types:

```rust,ignore
pub trait KabuNode<P, DB>: Send + Sync + 'static 
where
    P: Provider + Send + Sync + 'static,
    DB: Database + Send + Sync + 'static,
{
    async fn build_components(
        &self,
        context: &KabuBuildContext<P, DB>,
    ) -> Result<Vec<BoxedComponent>>;
}
```

### KabuEthereumNode

The standard implementation that includes all MEV components:

```rust,ignore
pub struct KabuEthereumNode<P, DB> {
    _phantom: PhantomData<(P, DB)>,
}

impl<P, DB> KabuNode<P, DB> for KabuEthereumNode<P, DB> {
    async fn build_components(
        &self,
        context: &KabuBuildContext<P, DB>,
    ) -> Result<Vec<BoxedComponent>> {
        // Builds market, network, strategy, execution, 
        // monitoring, and broadcasting components
    }
}
```

## KabuBuilder

The builder provides a fluent API for node construction:

```rust,ignore
pub struct KabuBuilder<P, DB> {
    context: KabuBuildContext<P, DB>,
    node: Option<Box<dyn KabuNode<P, DB>>>,
}

impl<P, DB> KabuBuilder<P, DB> {
    pub fn new(context: KabuBuildContext<P, DB>) -> Self {
        Self { context, node: None }
    }
    
    pub fn node(mut self, node: impl KabuNode<P, DB>) -> Self {
        self.node = Some(Box::new(node));
        self
    }
    
    pub fn build(self) -> BuiltKabu<P, DB> {
        BuiltKabu {
            context: self.context,
            node: self.node.unwrap_or_else(|| 
                Box::new(KabuEthereumNode::default())
            ),
        }
    }
}
```

### Launching

The built node can be launched with a task executor:

```rust,ignore
impl<P, DB> BuiltKabu<P, DB> {
    pub async fn launch(self, executor: TaskExecutor) -> Result<KabuHandle> {
        // Build components from the node
        let components = self.node.build_components(&self.context).await?;
        
        // Spawn all components
        for component in components {
            component.spawn_boxed(executor.clone())?;
        }
        
        Ok(KabuHandle::new())
    }
}
```

## KabuBuildContext

The build context contains all shared resources and configuration:

```rust,ignore
pub struct KabuBuildContext<P, DB> {
    // Provider and blockchain
    pub provider: P,
    pub blockchain: Blockchain,
    pub blockchain_state: BlockchainState<DB, KabuDataTypesEthereum>,
    
    // Communication channels
    pub channels: MevComponentChannels<DB>,
    
    // Configuration
    pub topology_config: TopologyConfig,
    pub backrun_config: BackrunConfig,
    pub pools_config: PoolsLoadingConfig,
    
    // Infrastructure
    pub multicaller_address: Address,
    pub swap_encoder: MulticallerSwapEncoder,
    pub db_pool: Option<DbPool>,
    
    // Shared state
    pub market: Arc<RwLock<Market>>,
    pub market_state: Arc<RwLock<MarketState<DB>>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub block_history: Arc<RwLock<BlockHistory>>,
    pub latest_block: Arc<RwLock<LatestBlock>>,
    
    // Flags
    pub is_exex: bool,
    pub enable_web_server: bool,
}
```

### Builder Pattern

The context uses a builder for flexible initialization:

```rust,ignore
let context = KabuBuildContextBuilder::new(
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
.with_channels(channels)
.with_enable_web_server(false)
.build();
```

## Component Selection

KabuEthereumNode builds components based on configuration:

### Market Components
- **HistoryPoolLoaderComponent** - Loads pools from database
- **ProtocolPoolLoaderComponent** - Discovers new pools
- **MarketStatePreloadedComponent** - Initializes market state

### Network Components  
- **WaitForNodeSyncComponent** - Ensures node is synced
- **MempoolProcessingComponent** - Monitors mempool

### Strategy Components
- **StateChangeArbSearcherComponent** - Main arbitrage engine
- **Merger components** - Optimize swap paths

### Execution Components
- **EvmEstimatorComponent** - Gas estimation
- **SwapRouterComponent** - Transaction routing
- **SignersComponent** - Transaction signing

### Monitoring Components
- **PoolHealthMonitorComponent** - Tracks pool reliability
- **MetricsRecorderComponent** - Performance metrics
- **InfluxDbWriterComponent** - Metrics export

### Broadcasting Components
- **FlashbotsBroadcastComponent** - Flashbots submission
- **PublicBroadcastComponent** - Public mempool

## Custom Nodes

Create specialized nodes for different use cases:

### Research Node

```rust,ignore
pub struct ResearchNode;

impl<P, DB> KabuNode<P, DB> for ResearchNode {
    async fn build_components(
        &self,
        context: &KabuBuildContext<P, DB>,
    ) -> Result<Vec<BoxedComponent>> {
        let mut components = vec![];
        
        // Only market and monitoring components
        components.extend(build_market_components(context)?);
        components.extend(build_monitoring_components(context)?);
        
        // Custom research component
        components.push(Box::new(
            ResearchRecorderComponent::new()
                .with_channels(&context.channels)
        ));
        
        Ok(components)
    }
}
```

### Minimal Production Node

```rust,ignore
pub struct MinimalProductionNode;

impl<P, DB> KabuNode<P, DB> for MinimalProductionNode {
    async fn build_components(
        &self,
        context: &KabuBuildContext<P, DB>,
    ) -> Result<Vec<BoxedComponent>> {
        let mut components = vec![];
        
        // Only essential components
        components.push(Box::new(
            StateChangeArbSearcherComponent::new(
                context.backrun_config.clone()
            )
            .with_channels(&context.channels)
            .with_market_state(context.market_state.clone())
        ));
        
        components.push(Box::new(
            SignersComponent::new()
                .with_channels(&context.channels)
        ));
        
        components.push(Box::new(
            FlashbotsBroadcastComponent::new(
                context.provider.clone(),
                context.topology_config.actors.broadcaster.clone().unwrap(),
            )
            .with_channels(&context.channels)
        ));
        
        Ok(components)
    }
}
```

## Best Practices

1. **Resource Sharing**
   - Use `Arc<RwLock<_>>` for shared state
   - Clone channels from `MevComponentChannels`
   - Pass context by reference

2. **Component Selection**
   - Only include necessary components
   - Consider resource usage
   - Balance functionality vs performance

3. **Configuration**
   - Use topology config for external services
   - Use backrun config for strategy parameters
   - Use pools config to control pool loading

4. **Error Handling**
   - Components should handle their own errors
   - Use `spawn_critical` for essential components
   - Implement graceful shutdown

5. **Testing**
   - Test nodes with mock components
   - Use controlled environments
   - Verify component interactions