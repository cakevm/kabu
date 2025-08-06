# Custom messages
If you need to add new messages without modifying kabu, you can easily add a custom struct like `Blockchain` to keep the references for states and channels.

## Custom Blockchain
```rust,ignore
pub struct CustomBlockchain {
    custom_channel: Broadcaster<CustomMessage>,
}

impl CustomBlockchain {
    pub fn new() -> Self {
        Self {
            custom_channel: Broadcaster::new(10),
        }
    }
    pub fn custom_channel(&self) -> Broadcaster<CustomMessage> {
        self.custom_channel.clone()
    }
}
```

## Custom Component
Create a component that can use your custom blockchain:

```rust,ignore
pub struct ExampleComponent {
    custom_channel_rx: Option<broadcast::Sender<CustomMessage>>,
}

impl ExampleComponent {
    pub fn new() -> Self {
        Self {
            custom_channel_rx: None,
        }
    }
    
    pub fn on_custom_bc(self, custom_bc: &CustomBlockchain) -> Self {
        Self { 
            custom_channel_rx: Some(custom_bc.custom_channel()), 
            ..self 
        }
    }
}

impl Component for ExampleComponent {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let mut rx = self.custom_channel_rx
            .ok_or_else(|| eyre!("custom channel not set"))?
            .subscribe();
            
        executor.spawn_critical("ExampleComponent", async move {
            while let Ok(msg) = rx.recv().await {
                // Process custom messages
            }
        });
        
        Ok(())
    }
    
    fn name(&self) -> &'static str {
        "ExampleComponent"
    }
}
```

## Starting Custom Components
When loading your custom component, you can set the custom blockchain:

```rust,ignore
let custom_bc = CustomBlockchain::new();
let executor = TaskExecutor::new();

// Create and configure the component
let component = ExampleComponent::new()
    .on_custom_bc(&custom_bc);

// Spawn the component
component.spawn(executor)?;
```

## Integration with KabuComponentsBuilder
For more complex setups, you can extend the component builder pattern:

```rust,ignore
impl KabuComponentsBuilder {
    pub fn with_custom_components(self, custom_bc: &CustomBlockchain) -> Self {
        let component = ExampleComponent::new()
            .on_custom_bc(custom_bc);
            
        self.add_component(Box::new(component))
    }
}