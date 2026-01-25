//! Board API routes for remote board sync
//!
//! These routes allow remote clients to sync board data to/from JSON files
//! on the server.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use std::sync::Arc;

use super::AppState;
use crate::core::board::{BoardService, SyncResult};

/// Error response
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// Board API error
pub struct BoardApiError(anyhow::Error);

impl IntoResponse for BoardApiError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for BoardApiError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

type Result<T> = std::result::Result<T, BoardApiError>;

/// Export board to agent files
///
/// POST /api/board/{project_id}/export
///
/// Creates/updates JSON files for each island.
/// Returns the path to the board directory.
pub async fn export_board(
    State(_state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Json<String>> {
    let service = BoardService::new(project_id);

    // Use tokio::task::spawn_blocking for sync operations
    let board_dir = tokio::task::spawn_blocking(move || service.export_local_sync())
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

    Ok(Json(board_dir.to_string_lossy().to_string()))
}

/// Import board from agent files
///
/// POST /api/board/{project_id}/import
///
/// Reads JSON files and syncs them to the database.
/// Returns a summary of changes made.
pub async fn import_board(
    State(_state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Json<SyncResult>> {
    let service = BoardService::new(project_id);

    let result = tokio::task::spawn_blocking(move || service.import_local_sync())
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

    Ok(Json(result))
}

/// Get board directory path
///
/// GET /api/board/{project_id}/directory
pub async fn get_board_directory(
    State(_state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Json<String>> {
    let service = BoardService::new(project_id);
    Ok(Json(service.board_dir().to_string_lossy().to_string()))
}
