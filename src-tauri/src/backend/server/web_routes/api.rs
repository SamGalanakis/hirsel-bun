use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use lash::oauth;
use serde::{Deserialize, Serialize};

use crate::backend::config::LlmProvider;
use crate::backend::credentials::{
    resolve_codex_oauth_credentials, resolve_tavily_api_key, CredentialStore,
};
use crate::backend::{app, shepherd_runtime, ProjectStore, ShepherdThreadStore};

use super::super::AppState;
use super::support::{
    clear_cookie_header, effective_project_worker_image, load_project_page_state,
    load_thread_page_state, normalize_project_sandbox_image,
    persist_project_and_start_runtime_preparation, probe_project_create,
};

pub async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Serialize)]
struct ApiProject {
    id: i64,
    name: String,
    description: Option<String>,
    sandbox_image: Option<String>,
    created_at: String,
}

#[derive(Serialize)]
struct ApiPreparationStep {
    id: String,
    label: String,
    status: String,
    detail: Option<String>,
    progress: Option<f64>,
}

#[derive(Serialize)]
struct ApiProjectPreparation {
    project: ApiProject,
    worker_image: String,
    status: String,
    headline: String,
    detail: Option<String>,
    progress: f64,
    steps: Vec<ApiPreparationStep>,
    started_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct ApiProjectCreateProbe {
    normalized_repo_url: String,
    suggested_name: String,
    selected_branch: String,
    branch_source: String,
    has_root_flake: bool,
    worker_image: String,
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
    plan_progress: Option<ApiPlanProgress>,
}

#[derive(Serialize)]
struct ApiPlanProgress {
    completed: usize,
    total: usize,
}

#[derive(Serialize)]
struct ApiThread {
    id: String,
    project_id: i64,
    title: String,
    objective: String,
    summary: String,
    status: String,
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
    focus_html: Option<String>,
    focus_source: Option<String>,
}

#[derive(Serialize)]
struct ApiThreadPage {
    project: ApiProject,
    thread: ApiThread,
    history: Vec<ApiChatMessage>,
    activity: ApiScopeActivity,
    plan: Option<serde_json::Value>,
}

fn to_api_project(p: &crate::backend::project::Project) -> ApiProject {
    ApiProject {
        id: p.id,
        name: p.name.clone(),
        description: p.description.clone(),
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

fn to_api_preparation_step(
    step: &crate::backend::project::ProjectPreparationStep,
) -> ApiPreparationStep {
    ApiPreparationStep {
        id: step.id.clone(),
        label: step.label.clone(),
        status: step.status.clone(),
        detail: step.detail.clone(),
        progress: step.progress,
    }
}

fn to_api_project_preparation(
    project: &crate::backend::project::Project,
    preparation: &crate::backend::project::ProjectRuntimePreparation,
) -> ApiProjectPreparation {
    ApiProjectPreparation {
        project: to_api_project(project),
        worker_image: effective_project_worker_image(project.sandbox_image.as_deref()),
        status: preparation.status.clone(),
        headline: preparation.headline.clone(),
        detail: preparation.detail.clone(),
        progress: preparation.progress,
        steps: preparation
            .steps
            .iter()
            .map(to_api_preparation_step)
            .collect(),
        started_at: preparation.started_at.clone(),
        updated_at: preparation.updated_at.clone(),
    }
}

fn to_api_project_create_probe(
    probe: crate::backend::server::web_routes::support::ProjectCreateProbe,
) -> ApiProjectCreateProbe {
    ApiProjectCreateProbe {
        normalized_repo_url: probe.normalized_repo_url,
        suggested_name: probe.suggested_name,
        selected_branch: probe.selected_branch,
        branch_source: probe.branch_source,
        has_root_flake: probe.has_root_flake,
        worker_image: probe.worker_image,
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
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        last_activity_at: t.last_activity_at.clone(),
    }
}

/// Extract the latest plan snapshot from a thread's message history.
/// Walks messages in reverse looking for the last `update_plan` / `Plan Update` tool chunk
/// that contains a `plan` array in its input.
fn extract_latest_plan(
    messages: &[crate::backend::ShepherdChatMessage],
) -> Option<serde_json::Value> {
    use crate::backend::shepherd_runtime::ShepherdMessageChunk;
    for message in messages.iter().rev() {
        let Ok(chunks) = serde_json::from_str::<Vec<ShepherdMessageChunk>>(&message.chunks_json)
        else {
            continue;
        };
        for chunk in chunks.into_iter().rev() {
            let ShepherdMessageChunk::Tool { input, .. } = chunk else {
                continue;
            };
            let Some(input) = input else { continue };
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&input) else {
                continue;
            };
            if parsed.get("plan").and_then(|v| v.as_array()).is_some() {
                return Some(parsed);
            }
        }
    }
    None
}

// ── Handlers ──

pub async fn list_projects() -> Result<impl IntoResponse, (StatusCode, String)> {
    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let out: Vec<ApiProject> = projects.iter().map(to_api_project).collect();
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct ProjectCreateProbeQuery {
    repo_url: String,
    branch: Option<String>,
}

pub async fn probe_project_create_api(
    Query(query): Query<ProjectCreateProbeQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let probe = probe_project_create(&query.repo_url, query.branch.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(to_api_project_create_probe(probe)))
}

pub async fn get_project_preparation(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let state = crate::backend::ensure_project_runtime_preparation_started(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(to_api_project_preparation(&project, &state)))
}

pub async fn retry_project_preparation(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let state = crate::backend::retry_project_runtime_preparation(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(to_api_project_preparation(&project, &state)))
}

pub async fn get_project_page(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let page = load_project_page_state(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let focus_source = page
        .surface
        .focus_view
        .as_ref()
        .and_then(|view| view.source.clone());
    let focus_html = page
        .surface
        .focus_view
        .as_ref()
        .map(|view| view.html.clone());

    let out = ApiProjectPage {
        project: to_api_project(&page.project),
        projects: page.projects.iter().map(to_api_project).collect(),
        threads: page
            .threads
            .iter()
            .map(|tp| {
                let plan_progress = extract_latest_plan(&tp.history).and_then(|plan| {
                    let steps = plan.get("plan")?.as_array()?;
                    let total = steps.len();
                    let completed = steps
                        .iter()
                        .filter(|s| s.get("status").and_then(|v| v.as_str()) == Some("completed"))
                        .count();
                    Some(ApiPlanProgress { completed, total })
                });
                ApiThreadPanel {
                    thread: to_api_thread(&tp.thread),
                    history: tp.history.iter().map(to_api_message).collect(),
                    activity: to_api_activity(&tp.activity),
                    plan_progress,
                }
            })
            .collect(),
        history: page.history.iter().map(to_api_message).collect(),
        activity: to_api_activity(&page.activity),
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

    let plan = extract_latest_plan(&page.item.history);
    let out = ApiThreadPage {
        project: to_api_project(&page.project),
        thread: to_api_thread(&page.item.thread),
        history: page.item.history.iter().map(to_api_message).collect(),
        activity: to_api_activity(&page.item.activity),
        plan,
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
        workspace_path: None,
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
    if state.api_key.trim().is_empty() {
        let cookie = clear_cookie_header();
        let mut response = Json(serde_json::json!({ "ok": true })).into_response();
        response
            .headers_mut()
            .insert(axum::http::header::SET_COOKIE, cookie);
        return Ok(response);
    }

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
    codex_configured: bool,
    codex_source: Option<String>,
    tavily_required: bool,
    tavily_configured: bool,
    tavily_key_masked: Option<String>,
    tavily_source: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCodexDeviceStartResponse {
    status: String,
    device_auth_id: String,
    user_code: String,
    verify_url: String,
    interval: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCodexDevicePollRequest {
    device_auth_id: String,
    user_code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCodexDevicePollResponse {
    status: String,
    expires_at: Option<u64>,
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
    let codex = resolve_codex_oauth_credentials().await;
    let tavily = resolve_tavily_api_key().await;

    let provider = match config.llm.provider {
        LlmProvider::Codex => "codex",
        LlmProvider::Openrouter => "openrouter",
    };

    Ok(Json(ApiSettingsResponse {
        provider: provider.to_string(),
        openrouter_key_masked,
        openrouter_base_url: config.llm.openrouter_base_url,
        codex_configured: codex.is_some(),
        codex_source: codex.map(|value| value.source.as_str().to_string()),
        tavily_required: true,
        tavily_configured: tavily.is_some(),
        tavily_key_masked: tavily
            .as_ref()
            .map(|value| crate::backend::api_types::mask_credential(&value.api_key)),
        tavily_source: tavily.map(|value| value.source.as_str().to_string()),
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

pub async fn start_codex_device_flow(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    {
        let mut config = state.config.write().await;
        config.llm.provider = LlmProvider::Codex;
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let device = oauth::codex_request_device_code().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to start Codex device auth: {}", e),
        )
    })?;

    Ok(Json(ApiCodexDeviceStartResponse {
        status: "pending".to_string(),
        device_auth_id: device.device_auth_id,
        user_code: device.user_code,
        verify_url: oauth::CODEX_DEVICE_VERIFY_URL.to_string(),
        interval: device.interval,
    }))
}

pub async fn poll_codex_device_flow(
    Json(body): Json<ApiCodexDevicePollRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let polled = oauth::codex_poll_device_auth(&body.device_auth_id, &body.user_code)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to poll Codex device auth: {}", e),
            )
        })?;

    let Some((authorization_code, code_verifier)) = polled else {
        return Ok(Json(ApiCodexDevicePollResponse {
            status: "pending".to_string(),
            expires_at: None,
        }));
    };

    let tokens = oauth::codex_exchange_code(&authorization_code, &code_verifier)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to exchange Codex auth code: {}", e),
            )
        })?;

    let expires_at = tokens.expires_at;
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .store_codex_oauth(&crate::backend::credentials::CodexOAuthCredentials {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            account_id: tokens.account_id,
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ApiCodexDevicePollResponse {
        status: "connected".to_string(),
        expires_at: Some(expires_at),
    }))
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
    let api_key = body.api_key.trim();
    if api_key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Tavily is required. Enter a Tavily key or set TAVILY_API_KEY.".to_string(),
        ));
    }

    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .store("tavily_api_key", api_key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
