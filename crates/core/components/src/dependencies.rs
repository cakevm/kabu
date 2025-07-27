use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A container for component outputs that other components can depend on
#[derive(Clone, Default)]
pub struct Dependencies {
    outputs: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl Dependencies {
    pub fn new() -> Self {
        Self { outputs: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Store an output from a component
    pub async fn provide<T: Any + Send + Sync + 'static>(&self, value: T) {
        let mut outputs = self.outputs.write().await;
        outputs.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieve an output from another component
    pub async fn get<T: Any + Send + Sync + Clone + 'static>(&self) -> Option<T> {
        let outputs = self.outputs.read().await;
        outputs.get(&TypeId::of::<T>()).and_then(|v| v.downcast_ref::<T>()).cloned()
    }

    /// Wait for a dependency to become available
    pub async fn wait_for<T: Any + Send + Sync + Clone + 'static>(&self) -> T {
        loop {
            if let Some(value) = self.get::<T>().await {
                return value;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }
}

/// Extension to BuilderContext to include dependencies
#[derive(Clone)]
pub struct BuilderContextWithDeps<State> {
    pub state: State,
    pub deps: Dependencies,
}

impl<State> BuilderContextWithDeps<State> {
    pub fn new(state: State, deps: Dependencies) -> Self {
        Self { state, deps }
    }
}
