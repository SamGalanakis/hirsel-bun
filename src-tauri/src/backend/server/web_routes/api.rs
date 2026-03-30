use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::backend::config::LlmProvider;
use crate::backend::credentials::CredentialStore;
use crate::backend::{app, shepherd_runtime, ProjectStore, ShepherdThreadStore};

use super::super::AppState;
use super::support::{
    clear_cookie_header, cookie_headers, load_project_page_state, load_thread_page_state,
    normalize_project_sandbox_image, persist_project_and_start_runtime_preparation,
};

pub async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Serialize)]
struct ApiProject {
    id: i64,
    name: String,
    description: Option<String>,
    repo_url: String,
    branch: String,
    sandbox_image: Option<String>,
    created_at: String,
}

#[derive(Serialize)]
struct ApiChatMessage {
    id: i64,
    role: String,
    chunks_json: String,
    timestamp: String,
}

#[derive(Serialize)]
struct ApiLiveTurn {
    chunks_json: String,
    status: String,
    updated_at: String,
}

#[derive(Serialize)]
struct ApiScopeActivity {
    session: Option<ApiSession>,
    live_turn: Option<ApiLiveTurn>,
    has_active_turn: bool,
}

#[derive(Serialize)]
struct ApiSession {
    status: String,
    last_error: Option<String>,
}

#[derive(Serialize)]
struct ApiThreadPanel {
    thread: ApiThread,
    history: Vec<ApiChatMessage>,
    activity: ApiScopeActivity,
}

#[derive(Serialize)]
struct ApiThread {
    id: String,
    project_id: i64,
    title: String,
    objective: String,
    summary: String,
    status: String,
    workspace_path: Option<String>,
    checkout_name: Option<String>,
    created_at: String,
    updated_at: String,
    last_activity_at: String,
}

#[derive(Serialize)]
struct ApiProjectPage {
    project: ApiProject,
    projects: Vec<ApiProject>,
    threads: Vec<ApiThreadPanel>,
    history: Vec<ApiChatMessage>,
    activity: ApiScopeActivity,
    has_queued: bool,
    focus_html: Option<String>,
    focus_source: Option<String>,
}

#[derive(Serialize)]
struct ApiThreadPage {
    project: ApiProject,
    thread: ApiThread,
    history: Vec<ApiChatMessage>,
    activity: ApiScopeActivity,
    has_queued: bool,
}

fn to_api_project(p: &crate::backend::project::Project) -> ApiProject {
    ApiProject {
        id: p.id,
        name: p.name.clone(),
        description: p.description.clone(),
        repo_url: p.repo_url.clone(),
        branch: p.branch.clone(),
        sandbox_image: p.sandbox_image.clone(),
        created_at: p.created_at.clone(),
    }
}

fn to_api_message(m: &crate::backend::ShepherdChatMessage) -> ApiChatMessage {
    ApiChatMessage {
        id: m.id,
        role: m.role.clone(),
        chunks_json: m.chunks_json.clone(),
        timestamp: m.timestamp.clone(),
    }
}

fn to_api_activity(a: &shepherd_runtime::ShepherdScopeActivity) -> ApiScopeActivity {
    ApiScopeActivity {
        session: a.session.as_ref().map(|s| ApiSession {
            status: s.status.clone(),
            last_error: s.last_error.clone(),
        }),
        live_turn: a.live_turn.as_ref().map(|t| ApiLiveTurn {
            chunks_json: t.chunks_json.clone(),
            status: t.status.clone(),
            updated_at: t.updated_at.clone(),
        }),
        has_active_turn: a.has_active_turn,
    }
}

fn to_api_thread(t: &crate::backend::ShepherdThread) -> ApiThread {
    ApiThread {
        id: t.id.clone(),
        project_id: t.project_id,
        title: t.title.clone(),
        objective: t.objective.clone(),
        summary: t.summary.clone(),
        status: t.status.clone(),
        workspace_path: t.workspace_path.clone(),
        checkout_name: t.checkout_name.clone(),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        last_activity_at: t.last_activity_at.clone(),
    }
}

// ── Handlers ──

pub async fn list_projects() -> Result<impl IntoResponse, (StatusCode, String)> {
    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let out: Vec<ApiProject> = projects.iter().map(to_api_project).collect();
    Ok(Json(out))
}

pub async fn get_project_page(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let page = load_project_page_state(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let scope = shepherd_runtime::ShepherdScope::Project {
        project_id,
        workspace_path: None,
        focus: None,
    };
    let has_queued = shepherd_runtime::has_queued_turn(&scope);

    let focus_source = page.surface.focus_view.source.clone();
    let focus_html = if focus_source.as_deref() != Some("placeholder") {
        Some(page.surface.focus_view.html.clone())
    } else {
        None
    };

    let out = ApiProjectPage {
        project: to_api_project(&page.project),
        projects: page.projects.iter().map(to_api_project).collect(),
        threads: page
            .threads
            .iter()
            .map(|tp| ApiThreadPanel {
                thread: to_api_thread(&tp.thread),
                history: tp.history.iter().map(to_api_message).collect(),
                activity: to_api_activity(&tp.activity),
            })
            .collect(),
        history: page.history.iter().map(to_api_message).collect(),
        activity: to_api_activity(&page.activity),
        has_queued,
        focus_html,
        focus_source,
    };
    Ok(Json(out))
}

pub async fn get_thread_page(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let page = load_thread_page_state(project_id, &thread_id, 200)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let scope = shepherd_runtime::ShepherdScope::Thread {
        project_id,
        thread_id: page.item.thread.id.clone(),
        title: page.item.thread.title.clone(),
        workspace_path: page.item.thread.workspace_path.clone(),
        focus: None,
    };
    let has_queued = shepherd_runtime::has_queued_turn(&scope);

    let out = ApiThreadPage {
        project: to_api_project(&page.project),
        thread: to_api_thread(&page.item.thread),
        history: page.item.history.iter().map(to_api_message).collect(),
        activity: to_api_activity(&page.item.activity),
        has_queued,
    };
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct ChatSendBody {
    content: String,
}

pub async fn send_chat_message(
    Path(project_id): Path<i64>,
    Json(body): Json<ChatSendBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    shepherd_runtime::send_project_message(project_id, Some(body.content), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn stop_chat(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let scope = shepherd_runtime::ShepherdScope::Project {
        project_id,
        workspace_path: None,
        focus: None,
    };
    shepherd_runtime::interrupt_scope_turn(scope)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn send_thread_message(
    Path((project_id, thread_id)): Path<(i64, String)>,
    Json(body): Json<ChatSendBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    shepherd_runtime::send_thread_message(project_id, &thread_id, Some(body.content), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn stop_thread_chat(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let thread = thread_store
        .get_thread(&thread_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let scope = shepherd_runtime::ShepherdScope::Thread {
        project_id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        workspace_path: thread.workspace_path.clone(),
        focus: None,
    };
    shepherd_runtime::interrupt_scope_turn(scope)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ConnectBody {
    api_key: String,
}

pub async fn connect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ConnectBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if body.api_key != state.api_key {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_string()));
    }
    // Set session cookie
    let cookie = super::support::cookie_headers(&body.api_key);
    let mut response = Json(serde_json::json!({ "ok": true })).into_response();
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie);
    Ok(response)
}

pub async fn delete_project(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .delete_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Settings & project creation API handlers ──

#[derive(Serialize)]
struct ApiSettingsResponse {
    provider: String,
    openrouter_key_masked: Option<String>,
    openrouter_base_url: Option<String>,
    codex_connected: bool,
    tavily_key_masked: Option<String>,
}

pub async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let config = state.config.read().await.clone();
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let openrouter_key_masked = store
        .load("openrouter_api_key")
        .await
        .ok()
        .map(|value| crate::backend::api_types::mask_credential(&value));
    let tavily_key_masked = store
        .load("tavily_api_key")
        .await
        .ok()
        .map(|value| crate::backend::api_types::mask_credential(&value));
    let codex_connected = store.load_codex_oauth().await.ok().flatten().is_some();

    let provider = match config.llm.provider {
        LlmProvider::Codex => "codex",
        LlmProvider::Openrouter => "openrouter",
    };

    Ok(Json(ApiSettingsResponse {
        provider: provider.to_string(),
        openrouter_key_masked,
        openrouter_base_url: config.llm.openrouter_base_url,
        codex_connected,
        tavily_key_masked,
    }))
}

#[derive(Deserialize)]
pub struct SaveLlmProviderBody {
    provider: String,
}

pub async fn save_llm_provider(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveLlmProviderBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let provider = match body.provider.as_str() {
        "openrouter" => LlmProvider::Openrouter,
        _ => LlmProvider::Codex,
    };

    {
        let mut config = state.config.write().await;
        config.llm.provider = provider;
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SaveOpenrouterKeyBody {
    api_key: String,
    base_url: Option<String>,
}

pub async fn save_openrouter_key(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveOpenrouterKeyBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    {
        let mut config = state.config.write().await;
        config.llm.provider = LlmProvider::Openrouter;
        config.llm.openrouter_base_url = body.base_url.filter(|v| !v.trim().is_empty());
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !body.api_key.trim().is_empty() {
        store
            .store_openrouter_api_key(body.api_key.trim())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SaveTavilyKeyBody {
    api_key: String,
}

pub async fn save_tavily_key(
    Json(body): Json<SaveTavilyKeyBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !body.api_key.trim().is_empty() {
        store
            .store("tavily_api_key", body.api_key.trim())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct CreateProjectBody {
    name: String,
    repo_url: String,
    branch: Option<String>,
    sandbox_image: Option<String>,
}

pub async fn create_project_api(
    Json(body): Json<CreateProjectBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let sandbox_image = normalize_project_sandbox_image(body.sandbox_image.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let project = persist_project_and_start_runtime_preparation(
        body.name,
        body.repo_url,
        body.branch,
        sandbox_image,
    )
    .await?;

    Ok(Json(to_api_project(&project)))
}

#[derive(Deserialize)]
pub struct SaveProjectSettingsBody {
    name: String,
    description: Option<String>,
    sandbox_image: Option<String>,
}

pub async fn save_project_settings(
    Path(project_id): Path<i64>,
    Json(body): Json<SaveProjectSettingsBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let sandbox_image = normalize_project_sandbox_image(body.sandbox_image.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let project =
        app::update_project_settings(project_id, body.name, body.description, sandbox_image)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    Ok(Json(to_api_project(&project)))
}

pub async fn logout() -> Result<impl IntoResponse, (StatusCode, String)> {
    let cookie = clear_cookie_header();
    let mut response = Json(serde_json::json!({ "ok": true })).into_response();
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie);
    Ok(response)
}
