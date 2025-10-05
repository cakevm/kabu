//! Provider wrapper that adds canonical state notification capabilities
//!
//! This wrapper allows any provider to emit canonical state notifications,
//! which is essential for both testing and remote node connections.

use eyre::Result;
use reth_chain_state::{CanonStateNotification, CanonStateSubscriptions};
use reth_ethereum_primitives::EthPrimitives;
use reth_execution_types::Chain;
use reth_primitives::Block;
use reth_primitives_traits::block::RecoveredBlock;
use reth_storage_api::NodePrimitivesProvider;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::trace;

/// A wrapper around a provider that adds canonical state notification capabilities.
///
/// This is necessary because not all providers (like RpcBlockchainProvider) implement
/// CanonStateSubscriptions natively. This wrapper adds that capability by maintaining
/// a broadcast channel for canonical state notifications.
#[derive(Debug, Clone)]
pub struct KabuRethProviderWrapper<P> {
    /// The inner provider that implements the actual blockchain access
    inner: P,
    /// Sender for canonical state notifications
    pub canon_state_notification_sender: broadcast::Sender<CanonStateNotification<EthPrimitives>>,
}

impl<P> KabuRethProviderWrapper<P> {
    /// Create a new provider wrapper with canonical state notification support
    pub fn new(inner: P) -> Self {
        // Create a broadcast channel for canonical state notifications
        // Buffer size of 256 should be sufficient for most use cases
        let (canon_state_notification_sender, _) = broadcast::channel(256);
        Self { inner, canon_state_notification_sender }
    }

    /// Send a canonical commit notification for a single block
    ///
    /// The caller needs to provide the block and execution outcome in the appropriate format
    pub fn send_canonical_commit(
        &self,
        block_with_senders: RecoveredBlock<Block>,
        execution_outcome: reth_execution_types::ExecutionOutcome,
    ) -> Result<()> {
        // Create a chain with a single block
        let chain = Chain::new(vec![block_with_senders], execution_outcome, None);

        // Send the commit notification
        let notification = CanonStateNotification::Commit { new: Arc::new(chain) };

        // Ignore send errors (no receivers is ok)
        let _ = self.canon_state_notification_sender.send(notification);
        trace!("Sent canonical commit notification");

        Ok(())
    }

    /// Get access to the inner provider
    pub fn inner(&self) -> &P {
        &self.inner
    }

    /// Get a mutable reference to the inner provider
    pub fn inner_mut(&mut self) -> &mut P {
        &mut self.inner
    }

    /// Consume the wrapper and return the inner provider
    pub fn into_inner(self) -> P {
        self.inner
    }
}

// Implement CanonStateSubscriptions for the wrapper
impl<P> CanonStateSubscriptions for KabuRethProviderWrapper<P>
where
    P: NodePrimitivesProvider<Primitives = EthPrimitives> + Send + Sync,
{
    fn subscribe_to_canonical_state(&self) -> reth_chain_state::CanonStateNotifications<Self::Primitives> {
        self.canon_state_notification_sender.subscribe()
    }

    // canonical_state_stream has a default implementation in the trait
}

// Implement std::ops::Deref to allow transparent access to inner provider methods
impl<P> std::ops::Deref for KabuRethProviderWrapper<P> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Implement std::ops::DerefMut for mutable access
impl<P> std::ops::DerefMut for KabuRethProviderWrapper<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// Implement NodePrimitivesProvider by delegating to the inner provider
impl<P> NodePrimitivesProvider for KabuRethProviderWrapper<P>
where
    P: NodePrimitivesProvider,
{
    type Primitives = P::Primitives;
}
