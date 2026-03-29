use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use datastar::consts::ElementPatchMode;
use datastar::prelude::{PatchElements, PatchSignals};
use serde::Deserialize;

use super::auth;
use super::AppState;
use crate::backend::config::LlmProvider;
use crate::backend::credentials::CredentialStore;
use crate::backend::draft::StartingPoint;
use crate::backend::llm_provider::resolve_provider;
use crate::backend::project::Project;
use crate::backend::route::CreateRouteRepoRequest;
use crate::backend::server::routes::{CodexDeviceExchangeRequest, CodexDevicePollRequest};
use crate::backend::webui::{
    render_chat_panel, render_connect_page, render_empty_projects_page, render_focus_document,
    render_project_focus_stage, render_project_page, render_project_settings_page,
    render_settings_page, render_thread_detail_page, render_threads_panel, ThreadPanelState,
};
use crate::backend::{app, shepherd_runtime};

#[derive(Deserialize)]
pub struct ConnectQuery {
    pub return_to: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct AppHomeQuery {
    pub create_error: Option<String>,
    pub name: Option<String>,
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub confirm_create_branch: Option<String>,
    pub confirm_base_branch: Option<String>,
}

#[derive(Deserialize)]
pub struct ConnectForm {
    pub api_key: String,
    pub return_to: Option<String>,
}

#[derive(Deserialize)]
pub struct BootstrapQuery {
    pub api_key: String,
    pub return_to: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateProjectForm {
    pub name: String,
    pub repo_url: String,
    pub branch: Option<String>,
}

#[derive(Deserialize)]
pub struct ConfirmCreateProjectForm {
    pub name: String,
    pub repo_url: String,
    pub branch: String,
    pub base_branch: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateRouteForm {
    pub name: String,
}

#[derive(Deserialize)]
pub struct ChatSendForm {
    pub content: String,
}

#[derive(Deserialize)]
pub struct UpdateProjectForm {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateRouteForm {
    pub route_id: i64,
    pub time_limit_minutes: Option<i64>,
    pub human_in_the_loop: Option<String>,
    pub target_branch: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateProviderForm {
    pub provider: String,
    pub openrouter_base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct StoreApiKeyForm {
    pub api_key: String,
}

#[derive(Deserialize)]
pub struct SaveOpenrouterForm {
    pub api_key: String,
    pub openrouter_base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct CodexStreamQuery {
    pub device_auth_id: String,
    pub user_code: String,
}

fn cookie_headers(value: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{}={}; HttpOnly; Path=/; SameSite=Lax",
        auth::SESSION_COOKIE,
        value
    ))
    .expect("valid cookie header")
}

fn patch_elements(selector: &str, html: String) -> Event {
    PatchElements::new(html)
        .selector(selector)
        .mode(ElementPatchMode::Outer)
        .into()
}

fn patch_signals(signals: impl Into<String>) -> Event {
    PatchSignals::new(signals).into()
}

fn clear_cookie_header() -> HeaderValue {
    HeaderValue::from_static("hirsel_session=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax")
}

fn redirect_with_cookie(location: &str, cookie: HeaderValue) -> Response {
    let mut response = Redirect::to(location).into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

fn sanitize_return_to(value: Option<&str>) -> String {
    match value {
        Some(path) if path.starts_with("/app") => path.to_string(),
        _ => "/app".to_string(),
    }
}

fn humanize_llm_setup_error(error: &str) -> String {
    if error.contains("Codex OAuth not configured") {
        "Connect Codex before using projects.".to_string()
    } else if error.contains("OpenRouter API key not configured") {
        "Add an OpenRouter API key before using projects.".to_string()
    } else {
        error.to_string()
    }
}

fn settings_redirect(return_to: &str, error: Option<&str>) -> Response {
    let mut location = format!(
        "/app/settings?required=llm&return_to={}",
        urlencoding::encode(return_to)
    );
    if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        location.push_str("&error=");
        location.push_str(&urlencoding::encode(error));
    }
    Redirect::to(&location).into_response()
}

fn project_create_redirect(
    error: &str,
    name: Option<&str>,
    repo_url: Option<&str>,
    branch: Option<&str>,
) -> Response {
    let mut location = format!("/app?create_error={}", urlencoding::encode(error));
    if let Some(value) = name.filter(|value| !value.trim().is_empty()) {
        location.push_str("&name=");
        location.push_str(&urlencoding::encode(value));
    }
    if let Some(value) = repo_url.filter(|value| !value.trim().is_empty()) {
        location.push_str("&repo_url=");
        location.push_str(&urlencoding::encode(value));
    }
    if let Some(value) = branch.filter(|value| !value.trim().is_empty()) {
        location.push_str("&branch=");
        location.push_str(&urlencoding::encode(value));
    }
    Redirect::to(&location).into_response()
}

fn project_confirm_redirect(
    error: &str,
    name: &str,
    repo_url: &str,
    branch: &str,
    base_branch: Option<&str>,
) -> Response {
    let mut location = format!(
        "/app?create_error={}&name={}&repo_url={}&branch={}&confirm_create_branch=1",
        urlencoding::encode(error),
        urlencoding::encode(name),
        urlencoding::encode(repo_url),
        urlencoding::encode(branch),
    );
    if let Some(value) = base_branch.filter(|value| !value.trim().is_empty()) {
        location.push_str("&confirm_base_branch=");
        location.push_str(&urlencoding::encode(value));
    }
    Redirect::to(&location).into_response()
}

enum RemoteProjectValidation {
    Ready,
    ConfirmCreateBranch { base_branch: Option<String> },
}

fn validate_remote_project_source(
    repo_url: &str,
    branch: Option<&str>,
) -> std::result::Result<RemoteProjectValidation, String> {
    let parsed = crate::backend::git::parse_github_url(repo_url);
    if let Some(branch_name) = branch.map(str::trim).filter(|value| !value.is_empty()) {
        let exists = crate::backend::git::remote_branch_exists(&parsed.repo_url, branch_name)
            .map_err(|error| format!("Could not reach the repository: {}", error))?;
        if !exists {
            let visible_branches = crate::backend::git::list_remote_branches(&parsed.repo_url)
                .map_err(|error| format!("Could not reach the repository: {}", error))?;
            let base_branch = crate::backend::git::remote_default_branch(&parsed.repo_url)
                .map_err(|error| format!("Could not reach the repository: {}", error))?
                .or_else(|| visible_branches.first().cloned());
            return Ok(RemoteProjectValidation::ConfirmCreateBranch { base_branch });
        }
        return Ok(RemoteProjectValidation::Ready);
    }

    let branches = crate::backend::git::list_remote_branches(&parsed.repo_url)
        .map_err(|error| format!("Could not reach the repository: {}", error))?;
    if branches.is_empty() {
        return Err(
            "This repository has no visible branches yet. Push a branch before creating a project."
                .to_string(),
        );
    }

    Ok(RemoteProjectValidation::Ready)
}

async fn persist_project_and_start_survey(
    name: String,
    repo_url: String,
    branch: Option<String>,
) -> Result<Project, (StatusCode, String)> {
    let project = app::create_project(
        name,
        vec![CreateRouteRepoRequest {
            name: None,
            starting_point: StartingPoint::GitRepo {
                url: repo_url,
                branch,
            },
            target_branch: None,
        }],
        Some(0),
        None,
        None,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    if let Err(e) = shepherd_runtime::launch_project_survey_thread(project.id, None).await {
        tracing::warn!(project_id = project.id, error = %e, "auto survey thread launch failed after project creation");
    }

    Ok(project)
}

async fn ensure_llm_ready(
    state: &Arc<AppState>,
    return_to: &str,
) -> std::result::Result<(), Response> {
    match current_llm_setup_error(state).await {
        Some(error) => Err(settings_redirect(return_to, Some(&error))),
        None => Ok(()),
    }
}

async fn current_llm_setup_error(state: &Arc<AppState>) -> Option<String> {
    let config = state.config.read().await.clone();
    resolve_provider(&config)
        .await
        .err()
        .map(|error| humanize_llm_setup_error(&error))
}

fn provider_name(provider: LlmProvider) -> &'static str {
    match provider {
        LlmProvider::Codex => "codex",
        LlmProvider::Openrouter => "openrouter",
    }
}

async fn load_project_page_state(
    project_id: i64,
) -> Result<
    (
        Vec<Project>,
        Project,
        crate::backend::route::Route,
        crate::backend::project::ProjectSurfaceSnapshot,
        Vec<ThreadPanelState>,
        Vec<crate::backend::ShepherdChatMessage>,
        shepherd_runtime::ShepherdQueueState,
        crate::backend::app::types::UnreadNotificationsResponse,
    ),
    String,
> {
    let projects = app::list_projects().await?;
    let project = projects
        .iter()
        .find(|item| item.id == project_id)
        .cloned()
        .ok_or_else(|| format!("Unknown project {}", project_id))?;
    let route = app::get_active_route(project_id).await?;
    let surface = app::get_project_surface(project_id).await?;
    let threads = shepherd_runtime::get_route_threads(project_id, route.id).await?;
    let thread_panels = threads
        .into_iter()
        .map(|thread| async {
            let history = shepherd_runtime::get_thread_conversation(
                project_id,
                route.id,
                &thread.id,
                &thread.title,
                32,
            )
            .await?;
            let queue =
                shepherd_runtime::get_thread_queue(project_id, route.id, &thread.id, &thread.title)
                    .await?;
            Ok::<ThreadPanelState, String>(ThreadPanelState {
                thread,
                history,
                queue,
            })
        })
        .collect::<Vec<_>>();
    let mut thread_panels_resolved = Vec::with_capacity(thread_panels.len());
    for item in thread_panels {
        thread_panels_resolved.push(item.await?);
    }
    let history = shepherd_runtime::get_route_conversation(project_id, route.id).await?;
    let queue = shepherd_runtime::get_route_queue(project_id, route.id).await?;
    let notifications = app::get_all_unread_notifications().await?;
    Ok((
        projects,
        project,
        route,
        surface,
        thread_panels_resolved,
        history,
        queue,
        notifications,
    ))
}

pub fn build_web_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(app_root))
        .route("/connect", get(connect_page))
        .route("/connect/session", post(connect_session))
        .route("/connect/bootstrap", get(connect_bootstrap))
        .route("/connect/logout", post(connect_logout))
        .route("/static/webui.css", get(webui_css))
        .route("/static/datastar.js", get(datastar_bundle))
        .route("/app", get(app_home))
        .route("/app/projects", post(create_project))
        .route("/app/projects/confirm-create", post(confirm_create_project))
        .route("/app/settings", get(settings_page))
        .route("/app/settings/llm", post(save_llm_settings))
        .route("/app/settings/openrouter", post(save_openrouter_key))
        .route("/app/settings/tavily", post(save_tavily_key))
        .route("/app/settings/codex/start", post(start_codex_device))
        .route("/app/settings/codex/stream", get(stream_codex_device))
        .route("/app/projects/{project_id}", get(project_page))
        .route("/app/projects/{project_id}/focus", get(project_focus_page))
        .route("/app/projects/{project_id}/stream", get(project_stream))
        .route(
            "/app/projects/{project_id}/chat/send",
            post(send_chat_message),
        )
        .route(
            "/app/projects/{project_id}/threads/{thread_id}",
            get(thread_detail_page),
        )
        .route(
            "/app/projects/{project_id}/threads/{thread_id}/stream",
            get(thread_detail_stream),
        )
        .route("/app/projects/{project_id}/routes", post(create_route))
        .route(
            "/app/projects/{project_id}/routes/{route_id}/select",
            post(select_route),
        )
        .route(
            "/app/projects/{project_id}/routes/{route_id}/archive",
            post(archive_route),
        )
        .route(
            "/app/projects/{project_id}/settings",
            get(project_settings_page),
        )
        .route(
            "/app/projects/{project_id}/settings/project",
            post(save_project_settings),
        )
        .route(
            "/app/projects/{project_id}/settings/route",
            post(save_route_settings),
        )
        .route("/app/projects/{project_id}/delete", post(delete_project))
}

pub async fn app_root() -> Redirect {
    Redirect::to("/app")
}

pub async fn connect_page(Query(query): Query<ConnectQuery>) -> impl IntoResponse {
    render_connect_page(query.error.as_deref(), query.return_to.as_deref())
}

pub async fn connect_session(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ConnectForm>,
) -> Response {
    if form.api_key != state.api_key {
        let return_to = sanitize_return_to(form.return_to.as_deref());
        return Redirect::to(&format!(
            "/connect?error=Invalid%20API%20key&return_to={}",
            urlencoding::encode(&return_to)
        ))
        .into_response();
    }

    redirect_with_cookie(
        &sanitize_return_to(form.return_to.as_deref()),
        cookie_headers(&form.api_key),
    )
}

pub async fn connect_bootstrap(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BootstrapQuery>,
) -> Response {
    if query.api_key != state.api_key {
        return Redirect::to("/connect?error=Invalid%20API%20key").into_response();
    }

    redirect_with_cookie(
        &sanitize_return_to(query.return_to.as_deref()),
        cookie_headers(&query.api_key),
    )
}

pub async fn connect_logout() -> Response {
    redirect_with_cookie("/connect", clear_cookie_header())
}

pub async fn webui_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("webui.css"),
    )
}

pub async fn datastar_bundle() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("vendor/datastar.js"),
    )
}

pub async fn app_home(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AppHomeQuery>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app").await {
        return Ok(response);
    }

    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(project) = projects.first() {
        Ok(Redirect::to(&format!("/app/projects/{}", project.id)).into_response())
    } else {
        Ok(render_empty_projects_page(
            &projects,
            query.create_error.as_deref(),
            query.name.as_deref(),
            query.repo_url.as_deref(),
            query.branch.as_deref(),
            query.confirm_create_branch.as_deref().is_some(),
            query.confirm_base_branch.as_deref(),
        )
        .into_response())
    }
}

pub async fn create_project(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateProjectForm>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app").await {
        return Ok(response);
    }

    let parsed = crate::backend::git::parse_github_url(&form.repo_url);
    let requested_branch = form
        .branch
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or(parsed.branch.clone());

    match validate_remote_project_source(&parsed.repo_url, requested_branch.as_deref()) {
        Ok(RemoteProjectValidation::Ready) => {}
        Ok(RemoteProjectValidation::ConfirmCreateBranch { base_branch }) => {
            let requested = requested_branch.as_deref().unwrap_or("main");
            let error = match base_branch.as_deref() {
                Some(base) => format!(
                    "Branch '{}' does not exist yet. Create it from '{}'?",
                    requested, base
                ),
                None => format!(
                    "Repository has no visible branches yet. Initialize it with branch '{}'?",
                    requested
                ),
            };
            return Ok(project_confirm_redirect(
                &error,
                &form.name,
                &parsed.repo_url,
                requested,
                base_branch.as_deref(),
            ));
        }
        Err(error) => {
            return Ok(project_create_redirect(
                &error,
                Some(&form.name),
                Some(&parsed.repo_url),
                requested_branch.as_deref(),
            ));
        }
    }

    let project =
        persist_project_and_start_survey(form.name, parsed.repo_url, requested_branch).await?;

    Ok(Redirect::to(&format!("/app/projects/{}", project.id)).into_response())
}

pub async fn confirm_create_project(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ConfirmCreateProjectForm>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app").await {
        return Ok(response);
    }

    let parsed = crate::backend::git::parse_github_url(&form.repo_url);
    let branch = form.branch.trim().to_string();
    let base_branch = form
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if let Err(error) =
        crate::backend::git::create_remote_branch(&parsed.repo_url, &branch, base_branch.as_deref())
    {
        let message = if let Some(base) = base_branch.as_deref() {
            format!(
                "Could not create branch '{}' from '{}': {}",
                branch, base, error
            )
        } else {
            format!(
                "Could not initialize the repository with branch '{}': {}",
                branch, error
            )
        };
        return Ok(project_confirm_redirect(
            &message,
            &form.name,
            &parsed.repo_url,
            &branch,
            base_branch.as_deref(),
        ));
    }

    let project =
        persist_project_and_start_survey(form.name, parsed.repo_url, Some(branch)).await?;

    Ok(Redirect::to(&format!("/app/projects/{}", project.id)).into_response())
}

pub async fn project_page(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let (projects, project, route, surface, threads, history, queue, notifications) =
        load_project_page_state(project_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(render_project_page(
        &projects,
        &project,
        &route,
        &surface,
        &threads,
        &history,
        &queue,
        &notifications,
    )
    .into_response())
}

pub async fn project_focus_page(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let surface = app::get_project_surface(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(
        axum::response::Html(render_focus_document(&surface.focus_view.html).into_string())
            .into_response(),
    )
}

pub async fn send_chat_message(
    Path(project_id): Path<i64>,
    Form(form): Form<ChatSendForm>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let stream = stream! {
        match shepherd_runtime::enqueue_project_message(project_id, Some(form.content.clone()), None).await {
            Ok(_) => {
                yield Ok::<Event, Infallible>(patch_signals("{chatDraft: '', chatError: ''}"));
            }
            Err(error) => {
                let message = if error.contains("Codex OAuth not configured") {
                    "Codex is not connected. Open Settings and connect Codex first.".to_string()
                } else if error.contains("OpenRouter API key not configured") {
                    "OpenRouter is not configured. Open Settings and add an API key first.".to_string()
                } else {
                    error
                };
                let escaped_message = serde_json::to_string(&message)
                    .unwrap_or_else(|_| "\"Failed to send message.\"".to_string());
                let escaped_draft = serde_json::to_string(&form.content)
                    .unwrap_or_else(|_| "\"\"".to_string());
                yield Ok::<Event, Infallible>(patch_signals(format!(
                    "{{chatDraft: {}, chatError: {}}}",
                    escaped_draft, escaped_message
                )));
            }
        }
    };
    Ok(Sse::new(stream))
}

pub async fn create_route(
    Path(project_id): Path<i64>,
    Form(form): Form<CreateRouteForm>,
) -> Result<Redirect, (StatusCode, String)> {
    app::create_route(project_id, form.name, None, None)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}", project_id)))
}

pub async fn select_route(
    Path((project_id, route_id)): Path<(i64, i64)>,
) -> Result<Redirect, (StatusCode, String)> {
    app::set_active_route(project_id, route_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}", project_id)))
}

pub async fn archive_route(
    Path((project_id, route_id)): Path<(i64, i64)>,
) -> Result<Redirect, (StatusCode, String)> {
    app::archive_route(project_id, route_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}", project_id)))
}

pub async fn settings_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let config = state.config.read().await.clone();
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let openrouter = store
        .load("openrouter_api_key")
        .await
        .ok()
        .map(|value| crate::backend::api_types::mask_credential(&value));
    let tavily = store
        .load("tavily_api_key")
        .await
        .ok()
        .map(|value| crate::backend::api_types::mask_credential(&value));
    let codex_connected = store.load_codex_oauth().await.ok().flatten().is_some();
    let codex_state = query
        .get("device_auth_id")
        .zip(query.get("user_code"))
        .zip(query.get("verify_url"))
        .map(|((device_auth_id, user_code), verify_url)| {
            (
                device_auth_id.as_str(),
                user_code.as_str(),
                verify_url.as_str(),
            )
        });
    let query_requires_setup = query
        .get("required")
        .map(|value| value == "llm")
        .unwrap_or(false);
    let llm_setup_error = current_llm_setup_error(&state).await;
    let setup_required = query_requires_setup || llm_setup_error.is_some();
    let setup_error_owned = query.get("error").cloned().or(llm_setup_error);
    let setup_error = setup_error_owned.as_deref();

    Ok(render_settings_page(
        provider_name(config.llm.provider),
        config.llm.openrouter_base_url.as_deref(),
        openrouter.as_deref(),
        codex_connected,
        tavily.as_deref(),
        codex_state,
        setup_required,
        setup_error,
    ))
}

pub async fn save_llm_settings(
    State(state): State<Arc<AppState>>,
    Form(form): Form<UpdateProviderForm>,
) -> Result<Redirect, (StatusCode, String)> {
    let provider = match form.provider.as_str() {
        "openrouter" => LlmProvider::Openrouter,
        _ => LlmProvider::Codex,
    };

    {
        let mut config = state.config.write().await;
        config.llm.provider = provider;
        config.llm.openrouter_base_url = form
            .openrouter_base_url
            .filter(|value| !value.trim().is_empty());
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let config = state.config.read().await.clone();
    if resolve_provider(&config).await.is_ok() {
        Ok(Redirect::to("/app"))
    } else {
        Ok(Redirect::to("/app/settings?required=llm"))
    }
}

pub async fn save_openrouter_key(
    State(state): State<Arc<AppState>>,
    Form(form): Form<SaveOpenrouterForm>,
) -> Result<Redirect, (StatusCode, String)> {
    {
        let mut config = state.config.write().await;
        config.llm.provider = LlmProvider::Openrouter;
        config.llm.openrouter_base_url = form
            .openrouter_base_url
            .filter(|value| !value.trim().is_empty());
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !form.api_key.trim().is_empty() {
        store
            .store_openrouter_api_key(form.api_key.trim())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    let config = state.config.read().await.clone();
    if resolve_provider(&config).await.is_ok() {
        Ok(Redirect::to("/app"))
    } else {
        Ok(Redirect::to("/app/settings?required=llm"))
    }
}

pub async fn save_tavily_key(
    Form(form): Form<StoreApiKeyForm>,
) -> Result<Redirect, (StatusCode, String)> {
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !form.api_key.trim().is_empty() {
        store
            .store("tavily_api_key", form.api_key.trim())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(Redirect::to("/app/settings"))
}

pub async fn start_codex_device(
    State(state): State<Arc<AppState>>,
) -> Result<Redirect, (StatusCode, String)> {
    {
        let mut config = state.config.write().await;
        config.llm.provider = LlmProvider::Codex;
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let response = crate::backend::server::routes::codex_device_start()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .0;
    Ok(Redirect::to(&format!(
        "/app/settings?required=llm&device_auth_id={}&user_code={}&verify_url={}",
        urlencoding::encode(&response.device_auth_id),
        urlencoding::encode(&response.user_code),
        urlencoding::encode(&response.verify_url),
    )))
}

pub async fn stream_codex_device(Query(query): Query<CodexStreamQuery>) -> impl IntoResponse {
    let stream = stream! {
        loop {
            let result = crate::backend::server::routes::codex_device_poll(axum::Json(CodexDevicePollRequest {
                device_auth_id: query.device_auth_id.clone(),
                user_code: query.user_code.clone(),
            })).await;

            if let Ok(axum::Json(response)) = result {
                if response.status == "approved" {
                    if let (Some(authorization_code), Some(code_verifier)) =
                        (response.authorization_code, response.code_verifier)
                    {
                        let _ = crate::backend::server::routes::codex_device_exchange(axum::Json(
                            CodexDeviceExchangeRequest {
                                authorization_code,
                                code_verifier,
                            },
                        ))
                        .await;
                        let html = maud::html! {
                            article class="card" id="codex-status" {
                                header {
                                    h3 { (crate::backend::icons::icon("key")) "Codex" }
                                    span class="pill status-working" { "Connected" }
                                }
                                section {
                                    p class="muted" { "Codex OAuth is connected." }
                                }
                            }
                        };
                        yield Ok::<Event, Infallible>(patch_elements("#codex-status", html.into_string()));
                        let header = maud::html! {
                            a href="/app" class="btn ghost" {
                                (crate::backend::icons::icon("arrow-left"))
                                "Open app"
                            }
                        };
                        yield Ok::<Event, Infallible>(patch_elements("#settings-header-action", header.into_string()));
                        yield Ok::<Event, Infallible>(patch_elements(
                            "#settings-setup-alert",
                            r#"<div id="settings-setup-alert"></div>"#.to_string(),
                        ));
                        break;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    };
    Sse::new(stream)
}

pub async fn project_settings_page(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}/settings", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let store = crate::backend::project::ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let route = app::get_active_route(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(render_project_settings_page(&project, &route).into_response())
}

pub async fn save_project_settings(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    Form(form): Form<UpdateProjectForm>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}/settings", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    app::update_project_name(project_id, form.name)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    app::update_project_description(project_id, form.description)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}/settings", project_id)).into_response())
}

pub async fn save_route_settings(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    Form(form): Form<UpdateRouteForm>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}/settings", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    app::update_route_settings(
        project_id,
        form.route_id,
        form.time_limit_minutes.filter(|value| *value > 0),
        form.human_in_the_loop.is_some(),
        form.target_branch.filter(|value| !value.trim().is_empty()),
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}/settings", project_id)).into_response())
}

pub async fn delete_project(Path(project_id): Path<i64>) -> Result<Redirect, (StatusCode, String)> {
    let store = crate::backend::project::ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .delete_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Redirect::to("/app"))
}

pub async fn thread_detail_page(
    State(state): State<Arc<AppState>>,
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}/threads/{}", project_id, thread_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let (_projects, project, route, _surface, threads, _history, _queue, _notifications) =
        load_project_page_state(project_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let item = threads
        .into_iter()
        .find(|item| item.thread.id == thread_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Unknown thread {}", thread_id),
            )
        })?;
    Ok(render_thread_detail_page(&project, &route, &item).into_response())
}

pub async fn thread_detail_stream(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> impl IntoResponse {
    let stream = stream! {
        let mut last_html = String::new();
        loop {
            let rendered = match load_project_page_state(project_id).await {
                Ok((_projects, _project, _route, _surface, threads, _history, _queue, _notifications)) => {
                    let maybe_thread = threads.into_iter().find(|item| item.thread.id == thread_id);
                    if let Some(item) = maybe_thread {
                        maud::html! {
                            div id="thread-transcript" class="thread-transcript" {
                                @for message in &item.history {
                                    @let is_user = message.role == "user";
                                    div class=(if is_user { "chat-row user" } else { "chat-row assistant" }) {
                                        article class=(if is_user { "chat-message-user" } else { "chat-message-assistant" }) {
                                            div class="message-meta" {
                                                span class="message-author" { (if is_user { "shepherd" } else { "thread" }) }
                                                span class="message-time" { (crate::backend::webui::format_time(&message.timestamp)) }
                                            }
                                            (crate::backend::webui::render_message_fragments(&message.chunks_json))
                                        }
                                    }
                                }
                            }
                        }.into_string()
                    } else {
                        String::new()
                    }
                }
                Err(_) => String::new(),
            };

            if !rendered.is_empty() && rendered != last_html {
                last_html = rendered.clone();
                yield Ok::<Event, Infallible>(patch_elements("#thread-transcript", rendered));
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    Sse::new(stream)
}

pub async fn project_stream(Path(project_id): Path<i64>) -> impl IntoResponse {
    let stream = stream! {
        let mut last_focus = String::new();
        let mut last_chat = String::new();
        let mut last_threads = String::new();

        loop {
            if let Ok((_projects, project, _route, surface, threads, history, queue, _notifications)) =
                load_project_page_state(project_id).await
            {
                let focus_markup = render_project_focus_stage(&project, &surface).into_string();
                let chat_markup = render_chat_panel(project.id, &threads, &history, &queue).into_string();
                let threads_markup = render_threads_panel(&project, &threads).into_string();

                if focus_markup != last_focus {
                    last_focus = focus_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#focus-panel", focus_markup));
                }
                if chat_markup != last_chat {
                    last_chat = chat_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#chat-panel", chat_markup));
                }
                if threads_markup != last_threads {
                    last_threads = threads_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#threads-panel", threads_markup));
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Sse::new(stream)
}
