use crate::router::router;
use axum::Router;
use kabu_core_components::Component;

use kabu_core_blockchain::{Blockchain, BlockchainState};
use kabu_rpc_state::AppState;
use kabu_storage_db::DbPool;
use kabu_types_blockchain::KabuDataTypesEthereum;
use revm::{DatabaseCommit, DatabaseRef};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::info;

pub async fn start_web_server_worker<S, DB>(
    host: String,
    extra_router: Router<S>,
    bc: Blockchain,
    state: BlockchainState<DB, KabuDataTypesEthereum>,
    db_pool: DbPool,
    shutdown_token: CancellationToken,
) -> eyre::Result<()>
where
    DB: DatabaseRef<Error = kabu_evm_db::KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    S: Clone + Send + Sync + 'static,
    Router: From<Router<S>>,
{
    let app_state = AppState { db: db_pool, bc, state };
    let router = router(app_state);
    let router = router.merge(extra_router);

    // logging
    let router = router.layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default().include_headers(true)));

    info!("Webserver listening on {}", &host);
    let listener = TcpListener::bind(host).await?;
    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            shutdown_token.cancelled().await;
            info!("Shutting down webserver...");
        })
        .await?;

    Ok(())
}

pub struct WebServerComponent<S, DB: Clone + Send + Sync + 'static> {
    host: String,
    extra_router: Router<S>,
    shutdown_token: CancellationToken,
    db_pool: DbPool,
    bc: Option<Blockchain>,
    state: Option<BlockchainState<DB, KabuDataTypesEthereum>>,
}

impl<S, DB> WebServerComponent<S, DB>
where
    DB: DatabaseRef<Error = kabu_evm_db::KabuDBError> + Send + Sync + Clone + Default + 'static,
    S: Clone + Send + Sync + 'static,
    Router: From<Router<S>>,
{
    pub fn new(host: String, extra_router: Router<S>, db_pool: DbPool, shutdown_token: CancellationToken) -> Self {
        Self { host, extra_router, shutdown_token, db_pool, bc: None, state: None }
    }

    pub fn on_bc(self, bc: &Blockchain, state: &BlockchainState<DB, KabuDataTypesEthereum>) -> Self {
        Self { bc: Some(bc.clone()), state: Some(state.clone()), ..self }
    }
}

impl<S, DB> Component for WebServerComponent<S, DB>
where
    S: Clone + Send + Sync + 'static,
    DB: DatabaseRef<Error = kabu_evm_db::KabuDBError> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
    Router: From<Router<S>>,
{
    fn spawn(self, executor: reth_tasks::TaskExecutor) -> eyre::Result<()> {
        let name = self.name();
        let bc = self.bc.ok_or_else(|| eyre::eyre!("Blockchain not set"))?;
        let state = self.state.ok_or_else(|| eyre::eyre!("State not set"))?;

        executor.spawn_critical(name, async move {
            if let Err(e) = start_web_server_worker(self.host, self.extra_router, bc, state, self.db_pool, self.shutdown_token).await {
                tracing::error!("Web server worker failed: {}", e);
            }
        });
        Ok(())
    }

    fn spawn_boxed(self: Box<Self>, executor: reth_tasks::TaskExecutor) -> eyre::Result<()> {
        (*self).spawn(executor)
    }

    fn name(&self) -> &'static str {
        "WebServerComponent"
    }
}
