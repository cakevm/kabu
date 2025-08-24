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
use tracing::error;

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

pub async fn chain_notifications_forwarder<P, Node>(
    executor: &TaskExecutor,
    provider: P,
    reth_provider: RpcBlockchainProvider<P, Node>,
) -> eyre::Result<()>
where
    P: RethApi + Send + Sync + 'static,
    Node: NodeTypes<Primitives = EthPrimitives>,
{
    let chain_notifications = match provider.subscribe_chain_notifications().await {
        Ok(subscription) => subscription,
        Err(err) => {
            error!(?err, "Failed to subscribe to chain notifications");
            return Err(eyre::eyre!("Subscription failed"));
        }
    };
    // Spawn task to forward canon state notifications from node to the reth provider
    executor.spawn_critical("chain-notifications-forwarder", async move {
        let mut stream = chain_notifications.into_stream();
        while let Some(canon_state_notification) = stream.next().await {
            if let Err(err) = reth_provider.canon_state_notification().send(canon_state_notification) {
                error!(?err, "Failed to send canon state notification");
            }
        }
    });

    Ok(())
}
