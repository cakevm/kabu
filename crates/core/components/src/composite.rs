use crate::Component;
use eyre::Result;
use reth_tasks::TaskExecutor;
use std::future::Future;
use std::pin::Pin;

/// A component that manages multiple sub-components
pub struct CompositeComponent {
    name: &'static str,
    components: Vec<Box<dyn Component>>,
}

impl CompositeComponent {
    pub fn new(name: &'static str) -> Self {
        Self { name, components: Vec::new() }
    }

    /// Add a sub-component
    pub fn with<C: Component + 'static>(mut self, component: C) -> Self {
        self.components.push(Box::new(component));
        self
    }

    /// Add multiple sub-components
    pub fn with_all<I>(mut self, components: I) -> Self
    where
        I: IntoIterator<Item = Box<dyn Component>>,
    {
        self.components.extend(components);
        self
    }
}

impl Component for CompositeComponent {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name;
        let component_count = self.components.len();

        tracing::info!("Spawning composite component {} with {} sub-components", name, component_count);

        // Spawn each component
        for component in self.components {
            let comp_name = component.name();
            tracing::info!("Spawning sub-component: {}", comp_name);
            component.spawn_boxed(executor.clone())?;
        }

        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

/// Builder for creating groups of similar components
pub struct ComponentGroup<B> {
    builders: Vec<B>,
    name: &'static str,
}

impl<B> ComponentGroup<B> {
    pub fn new(name: &'static str) -> Self {
        Self { builders: Vec::new(), name }
    }

    /// Add a builder to the group
    pub fn with(mut self, builder: B) -> Self {
        self.builders.push(builder);
        self
    }

    /// Add multiple builders
    pub fn with_all<I>(mut self, builders: I) -> Self
    where
        I: IntoIterator<Item = B>,
    {
        self.builders.extend(builders);
        self
    }

    /// Build all components in the group
    pub async fn build_all<State, C, F>(self, ctx: &crate::BuilderContext<State>, build_fn: F) -> Result<Vec<C>>
    where
        F: Fn(B, &crate::BuilderContext<State>) -> Pin<Box<dyn Future<Output = Result<C>> + Send>>,
        C: Component,
    {
        let mut components = Vec::new();
        for builder in self.builders {
            let component = build_fn(builder, ctx).await?;
            components.push(component);
        }
        Ok(components)
    }

    /// Build and spawn all components as a composite
    pub async fn build_composite<State, C, F>(self, ctx: &crate::BuilderContext<State>, build_fn: F) -> Result<CompositeComponent>
    where
        F: Fn(B, &crate::BuilderContext<State>) -> Pin<Box<dyn Future<Output = Result<C>> + Send>>,
        C: Component + 'static,
    {
        let name = self.name;
        let components = self.build_all(ctx, build_fn).await?;
        let mut composite = CompositeComponent::new(name);
        for component in components {
            composite = composite.with(component);
        }
        Ok(composite)
    }
}
