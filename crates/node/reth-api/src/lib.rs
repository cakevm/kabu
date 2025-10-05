mod provider_wrapper;
mod traits;

use alloy_provider::Provider;
use alloy_transport::TransportResult;
use async_trait::async_trait;
use futures_util::StreamExt;
use reth_chain_state::CanonStateNotification;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodeTypes;
use reth_storage_rpc_provider::RpcBlockchainProvider;
use reth_tasks::TaskExecutor;
use tracing::{error, info, warn};

pub use provider_wrapper::KabuRethProviderWrapper;
pub use traits::KabuRethFullProvider;

#[async_trait]
pub trait RethApi: Send + Sync {
    async fn subscribe_chain_notifications(&self) -> TransportResult<alloy_pubsub::Subscription<CanonStateNotification<EthPrimitives>>>;
}

#[async_trait]
impl<P> RethApi for P
where
    P: Provider,
{
    async fn subscribe_chain_notifications(&self) -> TransportResult<alloy_pubsub::Subscription<CanonStateNotification<EthPrimitives>>> {
        self.root().client().pubsub_frontend().ok_or_else(alloy_transport::TransportErrorKind::pubsub_unavailable)?;

        let mut call = self.client().request("reth_subscribeChainNotifications", ());
        call.set_is_subscription();
        let id = call.await?;
        self.root().get_subscription(id).await
    }
}

pub async fn chain_notifications_forwarder<P, Node, N>(
    executor: &TaskExecutor,
    provider: P,
    reth_provider: RpcBlockchainProvider<P, Node, N>,
) -> eyre::Result<()>
where
    P: RethApi + Send + Sync + 'static,
    Node: NodeTypes<Primitives = EthPrimitives>,
    N: alloy_network::Network,
{
    // Try to subscribe to Reth-specific chain notifications
    match provider.subscribe_chain_notifications().await {
        Ok(chain_notifications) => {
            info!("Successfully subscribed to Reth chain notifications");

            // Spawn task to forward canon state notifications from node to the reth provider
            executor.spawn_critical("chain-notifications-forwarder", async move {
                let mut stream = chain_notifications.into_stream();
                while let Some(canon_state_notification) = stream.next().await {
                    if let Err(err) = reth_provider.canon_state_notification().send(canon_state_notification) {
                        error!(?err, "Failed to send canon state notification");
                    }
                }
                warn!("Chain notifications stream ended");
            });

            Ok(())
        }
        Err(err) => {
            // Log warning instead of error, as this is expected for non-Reth nodes
            warn!(
                ?err,
                "Failed to subscribe to Reth chain notifications. This is expected for non-Reth nodes. The node will continue without Reth-specific features."
            );

            // Don't fail completely - allow the system to continue
            // The BlockHistoryComponent and other components should handle missing notifications gracefully
            Ok(())
        }
    }
}
