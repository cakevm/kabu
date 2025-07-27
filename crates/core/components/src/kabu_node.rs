use crate::Component;
use eyre::Result;
use reth_tasks::TaskExecutor;

// Note: This file should only contain trait definitions and framework code
// Concrete implementations should be in the bin/kabu crate or other implementation crates

/// Simple placeholder component for testing the framework
pub struct PlaceholderComponent {
    name: &'static str,
}

impl PlaceholderComponent {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl Component for PlaceholderComponent {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name;
        executor.spawn_critical(name, async move {
            tracing::info!("{} placeholder component running", name);
            // Placeholder components just log and stay alive
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                tracing::debug!("{} placeholder component heartbeat", name);
            }
        });
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: TaskExecutor) -> Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        self.name
    }
}
