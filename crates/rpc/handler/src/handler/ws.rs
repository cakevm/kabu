use axum::extract::{ConnectInfo, State};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};

use crate::dto::block::{BlockHeader, WebSocketMessage};
use kabu_rpc_state::AppState;
use kabu_types_events::MarketEvents;
use revm::{DatabaseCommit, DatabaseRef};
use std::net::SocketAddr;
use tracing::{error, warn};

/// Handle websocket upgrade
pub async fn ws_handler<DB: DatabaseRef<Error = kabu_evm_db::KabuDBError> + DatabaseCommit + Send + Sync + Clone + 'static>(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(app_state): State<AppState<DB>>,
) -> impl IntoResponse {
    ws.on_failed_upgrade(move |e| {
        warn!("ws upgrade error: {} with {}", e, addr);
    })
    .on_upgrade(move |socket| on_upgrade(socket, addr, app_state))
}

/// Actual websocket statemachine (one will be spawned per connection)
async fn on_upgrade<DB: DatabaseRef + DatabaseCommit + Send + Sync + Clone + 'static>(
    mut socket: WebSocket,
    _who: SocketAddr,
    app_state: AppState<DB>,
) {
    let mut receiver = app_state.bc.market_events_channel().subscribe();

    while let Ok(event) = receiver.recv().await {
        // Only process BlockHeaderUpdate events for WebSocket streaming
        let (block_number, timestamp, base_fee, next_base_fee) = match event {
            MarketEvents::BlockHeaderUpdate { block_number, timestamp, base_fee, next_base_fee, .. } => {
                (block_number, timestamp, base_fee, next_base_fee)
            }
            _ => continue, // Skip non-block events
        };

        let ws_msg = WebSocketMessage::BlockHeader(BlockHeader {
            number: block_number,
            timestamp,
            base_fee_per_gas: Some(base_fee),
            next_block_base_fee: next_base_fee,
        });
        match serde_json::to_string(&ws_msg) {
            Ok(json) => {
                let _ = socket.send(Message::Text(json.into())).await;
            }
            Err(e) => {
                error!("Failed to serialize block header: {}", e);
            }
        }
    }
}
