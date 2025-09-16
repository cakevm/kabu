use crate::dto::block::BlockHeader;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use kabu_rpc_state::AppState;
use revm::{DatabaseCommit, DatabaseRef};

/// Get latest block
///
/// Get the latest block header
#[utoipa::path(
    get,
    path = "latest_block",
    tag = "block",
    tags = [],
    responses(
    (status = 200, description = "Todo item created successfully", body = BlockHeader),
    )
)]
pub async fn latest_block<DB: DatabaseRef + DatabaseCommit + Send + Sync + Clone + 'static>(
    State(app_state): State<AppState<DB>>,
) -> Result<Json<BlockHeader>, (StatusCode, String)> {
    {
        // Get latest block from block history
        let block_history_guard = app_state.state.block_history();
        let block_history = block_history_guard.read().await;
        let latest_block_number = block_history.latest_block_number;

        if let Some(block_hash) = block_history.get_block_hash_for_block_number(latest_block_number)
            && let Some(entry) = block_history.get_block_history_entry(&block_hash)
        {
            let ret = BlockHeader {
                number: entry.header.number,
                timestamp: entry.header.timestamp,
                base_fee_per_gas: entry.header.base_fee_per_gas,
                next_block_base_fee: 0,
            };
            return Ok(Json(ret));
        }

        Err((StatusCode::INTERNAL_SERVER_ERROR, "No block header found".to_string()))
    }
}
