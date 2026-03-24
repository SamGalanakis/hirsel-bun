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
use crate::core::config::LlmProvider;
use crate::core::credentials::CredentialStore;
use crate::core::draft::StartingPoint;
use crate::core::project::Project;
use crate::core::route::CreateRouteRepoRequest;
use crate::core::server::routes::{CodexDeviceExchangeRequest, CodexDevicePollRequest};
use crate::core::webui::{
    render_chat_panel, render_connect_page, render_empty_projects_page, render_focus_document,
    render_project_page, render_project_settings_page, render_settings_page,
    render_worker_detail_page,
};
use crate::gui::commands::concerns as gui_concerns;
use crate::gui::commands::events as gui_events;
use crate::gui::commands::projects as gui_projects;
use crate::gui::commands::routes as gui_routes;
use crate::gui::commands::shepherd::commands as gui_shepherd;
use crate::gui::commands::shepherd::types::ShepherdScope;
use crate::gui::commands::workers as gui_workers;
use crate::gui::commands::worktree as gui_worktree;

#[derive(Deserialize)]
pub struct ConnectQuery {
    pub return_to: Option<String>,
    pub error: Option<String>,
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
        crate::core::route::Route,
        crate::core::project::ProjectSurfaceSnapshot,
        Vec<crate::core::worktree::WorkItemTree>,
        Vec<crate::core::api_types::Worker>,
        Vec<crate::core::ShepherdChatMessage>,
        gui_shepherd::ShepherdQueueState,
        crate::gui::commands::types::UnreadNotificationsResponse,
    ),
    String,
> {
    let projects = gui_projects::list_projects().await?;
    let project = projects
        .iter()
        .find(|item| item.id == project_id)
        .cloned()
        .ok_or_else(|| format!("Unknown project {}", project_id))?;
    let route = gui_routes::get_active_route(project_id).await?;
    let surface = gui_projects::get_project_surface(project_id).await?;
    let work_tree = gui_worktree::get_route_work_tree(project_id, route.id)
        .await?
        .tree;
    let workers = gui_workers::get_route_workers(project_id, route.id).await?;
    let history = gui_shepherd::get_shepherd_history(
        ShepherdScope::Project {
            project_id,
            workspace_path: None,
            focus: None,
        },
        100,
    )
    .await?;
    let queue = gui_shepherd::get_shepherd_queue(ShepherdScope::Project {
        project_id,
        workspace_path: None,
        focus: None,
    })
    .await?;
    let notifications = gui_concerns::get_all_unread_notifications().await?;
    Ok((
        projects,
        project,
        route,
        surface,
        work_tree,
        workers,
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
        .route("/app/settings", get(settings_page))
        .route("/app/settings/llm", post(save_llm_settings))
        .route("/app/settings/openrouter", post(save_openrouter_key))
        .route("/app/settings/tavily", post(save_tavily_key))
        .route("/app/settings/codex/start", post(start_codex_device))
        .route("/app/settings/codex/stream", get(stream_codex_device))
        .route("/app/projects/{project_id}", get(project_page))
        .route("/app/projects/{project_id}/focus", get(project_focus_page))
        .route("/app/projects/{project_id}/stream", get(project_stream))
        .route("/app/projects/{project_id}/sync", post(start_sync))
        .route(
            "/app/projects/{project_id}/chat/send",
            post(send_chat_message),
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
        .route(
            "/app/projects/{project_id}/routes/{route_id}/workers/{worker_name}",
            get(worker_detail_page),
        )
        .route(
            "/app/projects/{project_id}/routes/{route_id}/workers/{worker_name}/stream",
            get(worker_detail_stream),
        )
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

pub async fn app_home() -> Result<Response, (StatusCode, String)> {
    let projects = gui_projects::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(project) = projects.first() {
        Ok(Redirect::to(&format!("/app/projects/{}", project.id)).into_response())
    } else {
        Ok(render_empty_projects_page(&projects).into_response())
    }
}

pub async fn create_project(
    Form(form): Form<CreateProjectForm>,
) -> Result<Redirect, (StatusCode, String)> {
    let project = gui_projects::create_project(
        form.name,
        vec![CreateRouteRepoRequest {
            name: None,
            starting_point: StartingPoint::GitRepo {
                url: form.repo_url,
                branch: form.branch.filter(|value| !value.trim().is_empty()),
            },
            target_branch: None,
        }],
        Some(0),
        None,
        None,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    Ok(Redirect::to(&format!("/app/projects/{}", project.id)))
}

pub async fn project_page(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (projects, project, route, surface, work_tree, workers, history, queue, notifications) =
        load_project_page_state(project_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(render_project_page(
        &projects,
        &project,
        &route,
        &surface,
        &work_tree,
        &workers,
        &history,
        &queue,
        &notifications,
    ))
}

pub async fn project_focus_page(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let surface = gui_projects::get_project_surface(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(axum::response::Html(
        render_focus_document(&surface.focus_view.html).into_string(),
    ))
}

pub async fn start_sync(Path(project_id): Path<i64>) -> Result<Redirect, (StatusCode, String)> {
    gui_shepherd::start_project_sync(project_id, None, true)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}", project_id)))
}

pub async fn send_chat_message(
    Path(project_id): Path<i64>,
    Form(form): Form<ChatSendForm>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    gui_shepherd::enqueue_shepherd_message_for_scope(
        ShepherdScope::Project {
            project_id,
            workspace_path: None,
            focus: None,
        },
        Some(form.content),
        None,
        None,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let stream = stream! {
        yield Ok::<Event, Infallible>(patch_signals("{chatDraft: ''}"));
    };
    Ok(Sse::new(stream))
}

pub async fn create_route(
    Path(project_id): Path<i64>,
    Form(form): Form<CreateRouteForm>,
) -> Result<Redirect, (StatusCode, String)> {
    gui_routes::create_route(project_id, form.name, None, None)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}", project_id)))
}

pub async fn select_route(
    Path((project_id, route_id)): Path<(i64, i64)>,
) -> Result<Redirect, (StatusCode, String)> {
    gui_routes::set_active_route(project_id, route_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}", project_id)))
}

pub async fn archive_route(
    Path((project_id, route_id)): Path<(i64, i64)>,
) -> Result<Redirect, (StatusCode, String)> {
    gui_routes::archive_route(project_id, route_id)
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
        .map(|value| crate::core::api_types::mask_credential(&value));
    let tavily = store
        .load("tavily_api_key")
        .await
        .ok()
        .map(|value| crate::core::api_types::mask_credential(&value));
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

    Ok(render_settings_page(
        provider_name(config.llm.provider),
        config.llm.openrouter_base_url.as_deref(),
        openrouter.as_deref(),
        codex_connected,
        tavily.as_deref(),
        codex_state,
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

    Ok(Redirect::to("/app/settings"))
}

pub async fn save_openrouter_key(
    Form(form): Form<StoreApiKeyForm>,
) -> Result<Redirect, (StatusCode, String)> {
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !form.api_key.trim().is_empty() {
        store
            .store_openrouter_api_key(form.api_key.trim())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(Redirect::to("/app/settings"))
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

pub async fn start_codex_device() -> Result<Redirect, (StatusCode, String)> {
    let response = crate::core::server::routes::codex_device_start()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .0;
    Ok(Redirect::to(&format!(
        "/app/settings?device_auth_id={}&user_code={}&verify_url={}",
        urlencoding::encode(&response.device_auth_id),
        urlencoding::encode(&response.user_code),
        urlencoding::encode(&response.verify_url),
    )))
}

pub async fn stream_codex_device(Query(query): Query<CodexStreamQuery>) -> impl IntoResponse {
    let stream = stream! {
        loop {
            let result = crate::core::server::routes::codex_device_poll(axum::Json(CodexDevicePollRequest {
                device_auth_id: query.device_auth_id.clone(),
                user_code: query.user_code.clone(),
            })).await;

            match result {
                Ok(axum::Json(response)) => {
                    if response.status == "approved" {
                        if let (Some(authorization_code), Some(code_verifier)) =
                            (response.authorization_code, response.code_verifier)
                        {
                            let _ = crate::core::server::routes::codex_device_exchange(axum::Json(
                                CodexDeviceExchangeRequest {
                                    authorization_code,
                                    code_verifier,
                                },
                            ))
                            .await;
                            let html = maud::html! {
                                article class="form-card" id="codex-status" {
                                    h2 { "Codex" }
                                    p { "Codex is connected." }
                                }
                            };
                            yield Ok::<Event, Infallible>(patch_elements("#codex-status", html.into_string()));
                            break;
                        }
                    }
                }
                Err(_) => {}
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    };
    Sse::new(stream)
}

pub async fn project_settings_page(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = crate::core::project::ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let route = gui_routes::get_active_route(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(render_project_settings_page(&project, &route))
}

pub async fn save_project_settings(
    Path(project_id): Path<i64>,
    Form(form): Form<UpdateProjectForm>,
) -> Result<Redirect, (StatusCode, String)> {
    gui_projects::update_project_name(project_id, form.name)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    gui_projects::update_project_description(project_id, form.description)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!(
        "/app/projects/{}/settings",
        project_id
    )))
}

pub async fn save_route_settings(
    Path(project_id): Path<i64>,
    Form(form): Form<UpdateRouteForm>,
) -> Result<Redirect, (StatusCode, String)> {
    gui_routes::update_route_settings(
        project_id,
        form.route_id,
        form.time_limit_minutes.filter(|value| *value > 0),
        form.human_in_the_loop.is_some(),
        form.target_branch.filter(|value| !value.trim().is_empty()),
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!(
        "/app/projects/{}/settings",
        project_id
    )))
}

pub async fn worker_detail_page(
    Path((project_id, route_id, worker_name)): Path<(i64, i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = crate::core::project::ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let route = gui_routes::get_route(project_id, route_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let workers = gui_workers::get_route_workers(project_id, route_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let worker = workers
        .into_iter()
        .find(|item| item.name == worker_name)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Unknown worker {}", worker_name),
            )
        })?;
    let events = gui_events::get_route_worker_events(
        project_id,
        route_id,
        worker_name.clone(),
        None,
        Some(200),
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
    .events;
    Ok(render_worker_detail_page(
        &project, &route, &worker, &events,
    ))
}

pub async fn worker_detail_stream(
    Path((project_id, route_id, worker_name)): Path<(i64, i64, String)>,
) -> impl IntoResponse {
    let stream = stream! {
        let mut last_html = String::new();
        loop {
            let rendered = match gui_events::get_route_worker_events(project_id, route_id, worker_name.clone(), None, Some(200)).await {
                Ok(response) => {
                    let markup = maud::html! {
                        section id="worker-events" class="worker-events" {
                            @for event in &response.events {
                                article class="event-row" {
                                    div class="message-meta" {
                                        span { (&event.event_type) }
                                        span { (&event.timestamp) }
                                    }
                                    @if let Some(content) = &event.content {
                                        pre class="message-body" { (content) }
                                    }
                                }
                            }
                        }
                    };
                    markup.into_string()
                }
                Err(_) => String::new(),
            };

            if !rendered.is_empty() && rendered != last_html {
                last_html = rendered.clone();
                yield Ok::<Event, Infallible>(patch_elements("#worker-events", rendered));
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    Sse::new(stream)
}

pub async fn project_stream(Path(project_id): Path<i64>) -> impl IntoResponse {
    let stream = stream! {
        let mut last_focus = String::new();
        let mut last_header = String::new();
        let mut last_chat = String::new();
        let mut last_work = String::new();
        let mut last_workers = String::new();

        loop {
            if let Ok((_projects, project, route, surface, work_tree, workers, history, queue, notifications)) =
                load_project_page_state(project_id).await
            {
                let focus_markup = maud::html! {
                    section id="focus-panel" class="focus-panel" {
                        @if !matches!(surface.focus_view.source.as_deref(), Some("placeholder" | "seed")) {
                            iframe title={ "Project focus for " (&project.name) } src={ "/app/projects/" (project.id) "/focus" } class="focus-frame" {}
                        } @else {
                            div class="empty-focus" {
                                p class="eyebrow" { "Project focus view" }
                                h2 { "Awaiting project focus" }
                                form action={ "/app/projects/" (project.id) "/sync" } method="post" {
                                    button type="submit" class="primary-btn" { "Sync" }
                                }
                            }
                        }
                    }
                }.into_string();

                let chat_markup = render_chat_panel(project.id, &history, &queue).into_string();
                let work_markup = maud::html! {
                    section id="work-panel" class="panel-card" {
                        div class="panel-header" {
                            h2 { "Work" }
                        }
                        (crate::core::webui::render_work_tree_nodes(&work_tree))
                    }
                }.into_string();
                let workers_markup = maud::html! {
                    section id="workers-panel" class="panel-card" {
                        div class="panel-header" { h2 { "Workers" } }
                        (crate::core::webui::render_worker_cards(&workers))
                    }
                }.into_string();

                let header_markup = maud::html! {
                    header id="project-header" class="topbar" {
                        div {
                            h1 { (&project.name) }
                            p class="muted small" { "Route " (&route.name) }
                        }
                        div class="topbar-actions" {
                            span class="notification-pill" { (notifications.notifications.len()) " unread" }
                            button type="button" class="ghost-btn" data-on:click="$machineryOpen = !$machineryOpen" {
                                "Machinery"
                            }
                        }
                    }
                }.into_string();

                if focus_markup != last_focus {
                    last_focus = focus_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#focus-panel", focus_markup));
                }
                if header_markup != last_header {
                    last_header = header_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#project-header", header_markup));
                }
                if chat_markup != last_chat {
                    last_chat = chat_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#chat-panel", chat_markup));
                }
                if work_markup != last_work {
                    last_work = work_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#work-panel", work_markup));
                }
                if workers_markup != last_workers {
                    last_workers = workers_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#workers-panel", workers_markup));
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Sse::new(stream)
}
