use crate::backend::{shepherd_runtime, ShepherdThread, ShepherdThreadStore};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use shepherd_runtime::SpawnThreadRequest;

use super::common::{
    extract_latest_plan, plan_progress_from_messages, to_api_activity, to_api_message,
    to_api_thread, ApiChatMessage, ApiThreadDetail, ApiThreadSummary,
};

#[derive(Deserialize)]
pub struct HistoryQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct ChatSendBody {
    content: String,
}

async fn load_thread_record(
    project_id: i64,
    thread_id: &str,
) -> Result<ShepherdThread, (StatusCode, String)> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let thread = store
        .get_thread(thread_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if thread.project_id != project_id {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "thread {} does not belong to project {}",
                thread_id, project_id
            ),
        ));
    }
    Ok(thread)
}

pub async fn list_threads(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let threads = shepherd_runtime::get_project_threads(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let mut out = Vec::with_capacity(threads.len());
    for thread in threads {
        let history =
            shepherd_runtime::get_thread_conversation(project_id, &thread.id, &thread.title, 64)
                .await
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        let activity = shepherd_runtime::get_thread_activity(project_id, &thread.id, &thread.title)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        out.push(ApiThreadSummary {
            thread: to_api_thread(&thread),
            activity: to_api_activity(&activity),
            plan_progress: plan_progress_from_messages(&history),
        });
    }
    Ok(Json(out))
}

pub async fn get_thread(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let thread = load_thread_record(project_id, &thread_id).await?;
    let history =
        shepherd_runtime::get_thread_conversation(project_id, &thread.id, &thread.title, 200)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let activity = shepherd_runtime::get_thread_activity(project_id, &thread.id, &thread.title)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(ApiThreadDetail {
        thread: to_api_thread(&thread),
        activity: to_api_activity(&activity),
        plan: extract_latest_plan(&history),
    }))
}

pub async fn get_thread_history(
    Path((project_id, thread_id)): Path<(i64, String)>,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let thread = load_thread_record(project_id, &thread_id).await?;
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let history =
        shepherd_runtime::get_thread_conversation(project_id, &thread.id, &thread.title, limit)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let out: Vec<ApiChatMessage> = history.iter().map(to_api_message).collect();
    Ok(Json(out))
}

pub async fn send_thread_message(
    Path((project_id, thread_id)): Path<(i64, String)>,
    Json(body): Json<ChatSendBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Capture preview before content is consumed
    let event_preview: String = body.content.chars().take(120).collect();
    let chunks =
        crate::backend::skills::enrich_chat_message_chunks(project_id, body.content, false)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    // Notify shepherd that user messaged a thread directly
    let _ = crate::backend::shepherd_events::insert_event(
        project_id,
        "user_thread_message",
        serde_json::json!({
            "thread_id": thread_id,
            "content": event_preview,
        }),
    )
    .await;

    let response =
        shepherd_runtime::send_thread_message(project_id, &thread_id, None, Some(chunks))
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(response))
}

pub async fn stop_thread_chat(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let thread = load_thread_record(project_id, &thread_id).await?;
    let scope = shepherd_runtime::ShepherdScope::Thread {
        project_id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        focus: None,
    };
    shepherd_runtime::interrupt_scope_turn(scope)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SpawnThreadBody {
    pub objective: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub binding_kind: Option<String>,
    #[serde(default)]
    pub binding_data: Option<String>,
}

pub async fn spawn_thread(
    Path(project_id): Path<i64>,
    Json(body): Json<SpawnThreadBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let request = SpawnThreadRequest {
        objective: body.objective,
        title: body.title,
        capabilities: body.capabilities,
        binding_kind: body.binding_kind,
        binding_data: body.binding_data,
    };
    let spawned = shepherd_runtime::spawn_thread(project_id, body.parent_id, request)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(spawned))
}

pub async fn inspect_thread(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let inspection = shepherd_runtime::inspect_thread(project_id, thread_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(inspection))
}

pub async fn merge_thread(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let outcome = shepherd_runtime::merge_thread(project_id, thread_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(outcome))
}

pub async fn merge_thread_retry(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let outcome = shepherd_runtime::merge_thread_retry(project_id, thread_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(outcome))
}

pub async fn discard_thread(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    shepherd_runtime::discard_thread(project_id, thread_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
