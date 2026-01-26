//! Board API routes for remote board sync
//!
//! These routes allow remote clients to sync board data to/from JSON files
//! on the server.
//!
//! ## Endpoints
//!
//! - POST /api/board/{project_id}/export - Export board to per-task files
//! - POST /api/board/{project_id}/import - Import board from per-task files
//! - GET /api/board/{project_id}/directory - Get board directory path
//! - GET /api/board/{project_id}/tasks - List task file slugs
//! - GET /api/board/{project_id}/tasks/{slug} - Get a task file
//! - POST /api/board/{project_id}/tasks/{slug} - Write a task file
//! - DELETE /api/board/{project_id}/tasks/{slug} - Delete a task file

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::AppState;
use crate::core::board::{
    BoardService, BoardStorage, ExportScope, LocalBoardStorage, SyncResult, TaskFile,
};

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

/// Export request body
#[derive(Debug, Deserialize)]
pub struct ExportRequest {
    #[serde(default)]
    pub scope: ExportScope,
}

/// Export board to agent files
///
/// POST /api/board/{project_id}/export
///
/// Creates/updates JSON files for each top-level task.
/// Returns the path to the board directory.
pub async fn export_board(
    State(_state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<String>> {
    let mut service = BoardService::new(project_id);
    let scope = req.scope;

    // Use tokio::task::spawn_blocking for sync operations
    let board_dir = tokio::task::spawn_blocking(move || service.export_local_sync(&scope))
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
    let mut service = BoardService::new(project_id);

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

// ========== PER-FILE ENDPOINTS ==========

/// List task file slugs
///
/// GET /api/board/{project_id}/tasks
pub async fn list_task_files(
    State(_state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<String>>> {
    let storage = LocalBoardStorage::new(project_id);
    let slugs = storage.list_task_files().await?;
    Ok(Json(slugs))
}

/// Get a task file
///
/// GET /api/board/{project_id}/tasks/{slug}
pub async fn get_task_file(
    State(_state): State<Arc<AppState>>,
    Path((project_id, slug)): Path<(i64, String)>,
) -> Result<Json<TaskFile>> {
    let storage = LocalBoardStorage::new(project_id);
    let task_file = storage.read_task_file(&slug).await?;

    match task_file {
        Some(file) => Ok(Json(file)),
        None => Err(BoardApiError(anyhow::anyhow!(
            "Task file not found: {}",
            slug
        ))),
    }
}

/// Write a task file
///
/// POST /api/board/{project_id}/tasks/{slug}
pub async fn write_task_file(
    State(_state): State<Arc<AppState>>,
    Path((project_id, slug)): Path<(i64, String)>,
    Json(task_file): Json<TaskFile>,
) -> Result<()> {
    let storage = LocalBoardStorage::new(project_id);
    storage.write_task_file(&slug, &task_file).await?;
    Ok(())
}

/// Delete a task file
///
/// DELETE /api/board/{project_id}/tasks/{slug}
pub async fn delete_task_file(
    State(_state): State<Arc<AppState>>,
    Path((project_id, slug)): Path<(i64, String)>,
) -> Result<()> {
    let storage = LocalBoardStorage::new(project_id);
    storage.delete_task_file(&slug).await?;
    Ok(())
}
