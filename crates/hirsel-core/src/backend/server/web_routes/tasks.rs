use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::backend::shepherd_runtime;
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

#[derive(Deserialize)]
pub struct DispatchTaskBody {
    mode: String,
    thread_id: Option<String>,
}

pub async fn dispatch_task(
    Path((project_id, task_id)): Path<(i64, String)>,
    Json(body): Json<DispatchTaskBody>,
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

    let _ = store
        .update_task(&task_id, None, Some("active"), None)
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

            if let Ok(thread_store) = crate::backend::ShepherdThreadStore::open().await {
                let _ = thread_store
                    .set_focused_task(&thread.id, Some(&task_id))
                    .await;
            }

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

#[derive(Deserialize)]
pub struct ReviewActionBody {
    action: String,
}

pub async fn review_action(
    Path((_project_id, task_id)): Path<(i64, String)>,
    Json(body): Json<ReviewActionBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = TaskStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let new_status = match body.action.as_str() {
        "approve" => "done",
        "replan" => "todo",
        "dismiss" => "todo",
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid action".to_string())),
    };

    let _ = store
        .update_task(&task_id, None, Some(new_status), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(
        serde_json::json!({ "ok": true, "status": new_status }),
    ))
}
