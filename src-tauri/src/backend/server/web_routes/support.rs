use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode};
use axum::response::sse::Event;
use axum::response::{IntoResponse, Redirect, Response};
use datastar::consts::ElementPatchMode;
use datastar::prelude::{PatchElements, PatchSignals};
use serde::Deserialize;

use crate::backend::config::LlmProvider;
use crate::backend::draft::StartingPoint;
use crate::backend::llm_provider::resolve_provider;
use crate::backend::project::{Project, ProjectRuntimePreparation, ProjectSurfaceSnapshot};
use crate::backend::webui::{ProjectCreateDraft, ProjectCreateReview, ThreadPanelState};
use crate::backend::{
    app, ensure_project_runtime_preparation_started, shepherd_runtime, ProjectStore,
    ShepherdChatMessage, ShepherdThreadStore,
};

use super::super::AppState;

#[derive(Deserialize)]
pub(super) struct CreateProjectForm {
    pub name: String,
    pub repo_url: String,
    pub branch: Option<String>,
    pub sandbox_image: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ConfirmCreateProjectForm {
    pub name: String,
    pub repo_url: String,
    pub branch: String,
    pub base_branch: Option<String>,
    pub sandbox_image: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ChatSendForm {
    pub content: String,
}

#[derive(Deserialize)]
pub(super) struct UpdateProjectForm {
    pub name: String,
    pub description: Option<String>,
    pub sandbox_image: Option<String>,
}

pub(super) struct ProjectPageState {
    pub projects: Vec<Project>,
    pub project: Project,
    pub surface: ProjectSurfaceSnapshot,
    pub threads: Vec<ThreadPanelState>,
    pub history: Vec<ShepherdChatMessage>,
    pub activity: shepherd_runtime::ShepherdScopeActivity,
}

pub(super) struct ProjectPreparationPageState {
    pub projects: Vec<Project>,
    pub project: Project,
    pub preparation: ProjectRuntimePreparation,
    pub effective_sandbox_image: String,
}

pub(super) struct ThreadPageState {
    pub project: Project,
    pub item: ThreadPanelState,
}

pub(super) enum RemoteProjectValidation {
    Ready,
    ConfirmCreateBranch { base_branch: Option<String> },
}

pub(super) fn patch_elements(selector: &str, html: String) -> Event {
    PatchElements::new(html)
        .selector(selector)
        .mode(ElementPatchMode::Outer)
        .into()
}

pub(super) fn patch_signals(signals: impl Into<String>) -> Event {
    PatchSignals::new(signals).into()
}

pub(super) fn cookie_headers(value: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{}={}; HttpOnly; Path=/; SameSite=Lax",
        crate::backend::server::auth::SESSION_COOKIE,
        value
    ))
    .expect("valid cookie header")
}

pub(super) fn clear_cookie_header() -> HeaderValue {
    HeaderValue::from_static("hirsel_session=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax")
}

pub(super) fn redirect_with_cookie(location: &str, cookie: HeaderValue) -> Response {
    let mut response = Redirect::to(location).into_response();
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie);
    response
}

pub(super) fn sanitize_return_to(value: Option<&str>) -> String {
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

pub(super) fn settings_redirect(return_to: &str, error: Option<&str>) -> Response {
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

pub(super) fn validate_remote_project_source(
    repo_url: &str,
    branch: Option<&str>,
) -> Result<RemoteProjectValidation, String> {
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

pub(super) fn current_default_sandbox_image() -> String {
    crate::backend::config::Config::load()
        .map(|(config, _)| config.sandbox.image)
        .unwrap_or_else(|_| crate::backend::sandbox::SandboxConfig::default().image)
}

pub(super) fn normalize_project_sandbox_image(raw: Option<&str>) -> Result<Option<String>, String> {
    let default_image = current_default_sandbox_image();
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.chars().any(char::is_whitespace) {
        return Err("Docker image overrides cannot contain whitespace.".to_string());
    }
    if value == default_image {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

pub(super) fn draft_from_form(
    form: &CreateProjectForm,
    error: Option<String>,
    review: Option<ProjectCreateReview>,
) -> ProjectCreateDraft {
    let default_sandbox_image = current_default_sandbox_image();
    let sandbox_image = form
        .sandbox_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_sandbox_image.clone());

    ProjectCreateDraft {
        error,
        name: form.name.clone(),
        repo_url: form.repo_url.clone(),
        branch: form
            .branch
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("main")
            .to_string(),
        default_sandbox_image,
        sandbox_image,
        review,
    }
}

pub(super) fn draft_from_confirm_form(
    form: &ConfirmCreateProjectForm,
    error: Option<String>,
    flake_detected: bool,
) -> ProjectCreateDraft {
    let default_sandbox_image = current_default_sandbox_image();
    let sandbox_image = form
        .sandbox_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_sandbox_image.clone());

    ProjectCreateDraft {
        error,
        name: form.name.clone(),
        repo_url: form.repo_url.clone(),
        branch: form.branch.clone(),
        default_sandbox_image,
        sandbox_image,
        review: Some(ProjectCreateReview {
            needs_remote_branch: true,
            base_branch: form.base_branch.clone(),
            flake_detected,
        }),
    }
}

pub(super) fn detect_remote_flake(repo_url: &str, branch: Option<&str>) -> Result<bool, String> {
    let Some(branch) = branch.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    crate::backend::git::remote_branch_has_flake(repo_url, branch)
        .map_err(|error| format!("Could not inspect the repository flake: {}", error))
}

pub(super) async fn persist_project_and_start_runtime_preparation(
    name: String,
    repo_url: String,
    branch: Option<String>,
    sandbox_image: Option<String>,
) -> Result<Project, (StatusCode, String)> {
    let project = app::create_project(
        name,
        StartingPoint::GitRepo {
            url: repo_url,
            branch,
        },
        sandbox_image,
        None,
        None,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    ensure_project_runtime_preparation_started(project.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(project)
}

pub(super) async fn ensure_llm_ready(
    state: &Arc<AppState>,
    return_to: &str,
) -> Result<(), Response> {
    match current_llm_setup_error(state).await {
        Some(error) => Err(settings_redirect(return_to, Some(&error))),
        None => Ok(()),
    }
}

pub(super) async fn current_llm_setup_error(state: &Arc<AppState>) -> Option<String> {
    let config = state.config.read().await.clone();
    resolve_provider(&config)
        .await
        .err()
        .map(|error| humanize_llm_setup_error(&error))
}

pub(super) fn humanize_chat_send_error(error: String) -> String {
    if error.contains("Codex OAuth not configured") {
        "Codex is not connected. Open Settings and connect Codex first.".to_string()
    } else if error.contains("OpenRouter API key not configured") {
        "OpenRouter is not configured. Open Settings and add an API key first.".to_string()
    } else {
        error
    }
}

pub(super) fn provider_name(provider: LlmProvider) -> &'static str {
    match provider {
        LlmProvider::Codex => "codex",
        LlmProvider::Openrouter => "openrouter",
    }
}

pub(super) async fn load_project_page_state(project_id: i64) -> Result<ProjectPageState, String> {
    let projects = app::list_projects().await?;
    let project = projects
        .iter()
        .find(|item| item.id == project_id)
        .cloned()
        .ok_or_else(|| format!("Unknown project {}", project_id))?;
    let surface = app::get_project_surface(project_id).await?;
    let threads = shepherd_runtime::get_project_threads(project_id).await?;
    let thread_panels = threads
        .into_iter()
        .map(|thread| async {
            let history = shepherd_runtime::get_thread_conversation(
                project_id,
                &thread.id,
                &thread.title,
                32,
            )
            .await?;
            let activity =
                shepherd_runtime::get_thread_activity(project_id, &thread.id, &thread.title)
                    .await?;
            Ok::<ThreadPanelState, String>(ThreadPanelState {
                thread,
                history,
                activity,
            })
        })
        .collect::<Vec<_>>();
    let mut resolved_threads = Vec::with_capacity(thread_panels.len());
    for item in thread_panels {
        resolved_threads.push(item.await?);
    }
    let history = shepherd_runtime::get_project_conversation(project_id).await?;
    let activity = shepherd_runtime::get_project_activity(project_id).await?;

    Ok(ProjectPageState {
        projects,
        project,
        surface,
        threads: resolved_threads,
        history,
        activity,
    })
}

pub(super) async fn load_project_preparation_state(
    project_id: i64,
) -> Result<ProjectPreparationPageState, String> {
    let projects = app::list_projects().await?;
    let project = projects
        .iter()
        .find(|item| item.id == project_id)
        .cloned()
        .ok_or_else(|| format!("Unknown project {}", project_id))?;
    let preparation = ensure_project_runtime_preparation_started(project_id).await?;
    let effective_sandbox_image = project
        .sandbox_image
        .clone()
        .unwrap_or_else(current_default_sandbox_image);

    Ok(ProjectPreparationPageState {
        projects,
        project,
        preparation,
        effective_sandbox_image,
    })
}

pub(super) async fn load_thread_page_state(
    project_id: i64,
    thread_id: &str,
    limit: usize,
) -> Result<ThreadPageState, String> {
    let project_store = ProjectStore::open()
        .await
        .map_err(|e| format!("failed to open project store: {}", e))?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(|e| format!("failed to load project {}: {}", project_id, e))?;
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    let thread = thread_store
        .get_thread(thread_id)
        .await
        .map_err(|e| format!("failed to load thread {}: {}", thread_id, e))?;

    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }

    let history =
        shepherd_runtime::get_thread_conversation(project_id, &thread.id, &thread.title, limit)
            .await?;
    let activity =
        shepherd_runtime::get_thread_activity(project_id, &thread.id, &thread.title).await?;

    Ok(ThreadPageState {
        project,
        item: ThreadPanelState {
            thread,
            history,
            activity,
        },
    })
}
