//! Task-board endpoints. Tasks are threads with `binding_kind = "task"`.
//! These routes surface that subset for the UI's planning board — there is
//! no separate `task` entity in the backend.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::backend::{shepherd_runtime, ShepherdThread, ShepherdThreadStore};

/// Public shape surfaced by the `/api/projects/:id/tasks` endpoints. It is a
/// projection of a `ShepherdThread` and intentionally carries only the
/// task-board fields the UI needs.
#[derive(Debug, Clone, Serialize)]
pub struct ApiTaskThread {
    pub id: String,
    pub project_id: i64,
    pub title: String,
    pub status: String,
    pub content: Option<String>,
    pub review_json: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ShepherdThread> for ApiTaskThread {
    fn from(thread: ShepherdThread) -> Self {
        Self {
            id: thread.id,
            project_id: thread.project_id,
            title: thread.title,
            status: thread.status,
            content: thread.content,
            review_json: thread.review_json,
            sort_order: thread.sort_order,
            created_at: thread.created_at,
            updated_at: thread.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateTaskBody {
    title: String,
    content: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTaskBody {
    title: Option<String>,
    status: Option<String>,
    content: Option<String>,
}

#[derive(Deserialize)]
pub struct ReorderTasksBody {
    task_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct DispatchTaskBody {
    mode: String,
    thread_id: Option<String>,
}

#[derive(Deserialize)]
pub struct ReviewActionBody {
    action: String,
}

async fn open_store() -> Result<ShepherdThreadStore, (StatusCode, String)> {
    ShepherdThreadStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn list_tasks(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = open_store().await?;
    let tasks = store
        .list_project_task_threads(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let out: Vec<ApiTaskThread> = tasks.into_iter().map(ApiTaskThread::from).collect();
    Ok(Json(out))
}

pub async fn create_task(
    Path(project_id): Path<i64>,
    Json(body): Json<CreateTaskBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = open_store().await?;
    let task = store
        .create_task_thread(project_id, &body.title, body.content.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ApiTaskThread::from(task)))
}

pub async fn update_task(
    Path((project_id, task_id)): Path<(i64, String)>,
    Json(body): Json<UpdateTaskBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = open_store().await?;
    let existing = store
        .get_thread(&task_id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    if existing.project_id != project_id {
        return Err((StatusCode::NOT_FOUND, "Task not found".to_string()));
    }
    let content_update = body.content.as_deref().map(Some);
    let updated = store
        .update_task_thread_fields(
            &task_id,
            body.title.as_deref(),
            body.status.as_deref(),
            content_update,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ApiTaskThread::from(updated)))
}

pub async fn delete_task(
    Path((_project_id, task_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = open_store().await?;
    store
        .delete_thread(&task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn reorder_tasks(
    Path(project_id): Path<i64>,
    Json(body): Json<ReorderTasksBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = open_store().await?;
    store
        .reorder_task_threads(project_id, &body.task_ids)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn dispatch_task(
    Path((project_id, task_id)): Path<(i64, String)>,
    Json(body): Json<DispatchTaskBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = open_store().await?;
    let task = store
        .get_thread(&task_id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    if task.project_id != project_id {
        return Err((StatusCode::NOT_FOUND, "Task not found".to_string()));
    }

    let _ = store
        .update_task_thread_fields(&task_id, None, Some("active"), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match body.mode.as_str() {
        "continue" => Ok(Json(serde_json::json!({
            "mode": "continue",
            "task_id": task_id,
            "thread_id": body.thread_id,
        }))),
        "new_thread" => {
            let thread = shepherd_runtime::create_thread(project_id, &task.title, &task.title)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

            let _ = store.set_focused_task(&thread.id, Some(&task_id)).await;

            if let Some(content) = &task.content {
                let msg = format!(
                    "# Task: {}\n\n## Plan\n\n{}\n\n---\n\nImplement this plan.",
                    task.title, content
                );
                let _ =
                    shepherd_runtime::send_thread_message(project_id, &thread.id, Some(msg), None)
                        .await;
            }

            Ok(Json(serde_json::json!({
                "mode": "new_thread",
                "task_id": task_id,
                "thread_id": thread.id,
            })))
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            "mode must be 'continue' or 'new_thread'".to_string(),
        )),
    }
}

pub async fn review_action(
    Path((_project_id, task_id)): Path<(i64, String)>,
    Json(body): Json<ReviewActionBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = open_store().await?;

    let new_status = match body.action.as_str() {
        "approve" => "done",
        "replan" => "todo",
        "dismiss" => "todo",
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid action".to_string())),
    };

    let _ = store
        .update_task_thread_fields(&task_id, None, Some(new_status), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(
        serde_json::json!({ "ok": true, "status": new_status }),
    ))
}
