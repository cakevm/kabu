//! Component builder trait

use eyre::Result;
use kabu_core_components::Component;

/// Trait for building components from context
pub trait ComponentBuilder<Ctx>: Sized + Send + Sync + 'static {
    /// The component type that will be created
    type Component: Component;

    /// Build the component from the given context
    fn build(self, ctx: &Ctx) -> Result<Self::Component>;
}

/// Marker struct for disabled components
pub struct DisabledComponent;

impl Component for DisabledComponent {
    fn spawn(self, _executor: reth_tasks::TaskExecutor) -> Result<()> {
        // No-op for disabled components
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Disabled"
    }
}

impl<Ctx> ComponentBuilder<Ctx> for DisabledComponent {
    type Component = DisabledComponent;

    fn build(self, _ctx: &Ctx) -> Result<Self::Component> {
        Ok(DisabledComponent)
    }
}
