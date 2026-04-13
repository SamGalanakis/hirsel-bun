use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::backend::tasks::TaskStore;

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

pub async fn list_tasks(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = TaskStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let tasks = store
        .list_project_tasks(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(tasks))
}

pub async fn create_task(
    Path(project_id): Path<i64>,
    Json(body): Json<CreateTaskBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = TaskStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let task = store
        .create_task(project_id, &body.title, body.content.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(task))
}

pub async fn update_task(
    Path((project_id, task_id)): Path<(i64, String)>,
    Json(body): Json<UpdateTaskBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = TaskStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let task = store
        .get_task(&task_id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    if task.project_id != project_id {
        return Err((StatusCode::NOT_FOUND, "Task not found".to_string()));
    }
    let content_update = body.content.as_ref().map(|c| Some(c.as_str()));
    let updated = store
        .update_task(
            &task_id,
            body.title.as_deref(),
            body.status.as_deref(),
            content_update,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(updated))
}

pub async fn delete_task(
    Path((_project_id, task_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = TaskStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .delete_task(&task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn reorder_tasks(
    Path(project_id): Path<i64>,
    Json(body): Json<ReorderTasksBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = TaskStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .reorder_tasks(project_id, &body.task_ids)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
