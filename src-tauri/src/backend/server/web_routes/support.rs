use axum::http::{HeaderValue, StatusCode};

use crate::backend::draft::StartingPoint;
use crate::backend::git::{
    list_remote_branches, parse_github_url, remote_branch_exists, remote_branch_has_flake,
    remote_default_branch,
};
use crate::backend::project::{Project, ProjectSurfaceSnapshot};
use crate::backend::{
    app, ensure_project_runtime_preparation_started, shepherd_runtime, ProjectStore,
    ShepherdChatMessage, ShepherdThread, ShepherdThreadStore,
};

// ── Types ──

pub(super) struct ThreadPanelState {
    pub thread: ShepherdThread,
    pub history: Vec<ShepherdChatMessage>,
    pub activity: shepherd_runtime::ShepherdScopeActivity,
}

pub(super) struct ProjectPageState {
    pub projects: Vec<Project>,
    pub project: Project,
    pub surface: ProjectSurfaceSnapshot,
    pub threads: Vec<ThreadPanelState>,
    pub history: Vec<ShepherdChatMessage>,
    pub activity: shepherd_runtime::ShepherdScopeActivity,
}

pub(super) struct ThreadPageState {
    pub project: Project,
    pub item: ThreadPanelState,
}

pub(super) struct ProjectCreateProbe {
    pub normalized_repo_url: String,
    pub suggested_name: String,
    pub selected_branch: String,
    pub branch_source: String,
    pub has_root_flake: bool,
    pub worker_image: String,
}

// ── Cookie helpers ──

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

// ── Project creation ──

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

pub(super) fn current_default_sandbox_image() -> String {
    crate::backend::config::Config::load()
        .map(|(config, _)| config.sandbox.image)
        .unwrap_or_else(|_| crate::backend::sandbox::SandboxConfig::default().image)
}

pub(super) fn effective_project_worker_image(project_image: Option<&str>) -> String {
    project_image
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(current_default_sandbox_image)
}

fn derive_project_name_from_repo_url(repo_url: &str) -> String {
    let trimmed = repo_url.trim().trim_end_matches('/');
    let last_segment = trimmed
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches(".git");
    if last_segment.is_empty() {
        "project".to_string()
    } else {
        last_segment.to_string()
    }
}

fn choose_detected_branch(repo_url: &str) -> Result<String, String> {
    if let Some(branch) = remote_default_branch(repo_url).map_err(|e| e.to_string())? {
        return Ok(branch);
    }

    let branches = list_remote_branches(repo_url).map_err(|e| e.to_string())?;
    branches
        .iter()
        .find(|branch| branch.as_str() == "main")
        .or_else(|| branches.iter().find(|branch| branch.as_str() == "master"))
        .cloned()
        .or_else(|| branches.first().cloned())
        .ok_or_else(|| "No visible remote branches were found for this repository.".to_string())
}

pub(super) fn probe_project_create(
    repo_url: &str,
    branch: Option<&str>,
) -> Result<ProjectCreateProbe, String> {
    let parsed = parse_github_url(repo_url);
    let normalized_repo_url = parsed.repo_url.trim().to_string();
    if normalized_repo_url.is_empty() {
        return Err("Repository URL is required.".to_string());
    }

    let explicit_branch = branch.map(str::trim).filter(|value| !value.is_empty());
    let url_branch = parsed.branch.as_deref().filter(|value| !value.is_empty());
    let (selected_branch, branch_source) = if let Some(value) = explicit_branch {
        (value.to_string(), "explicit".to_string())
    } else if let Some(value) = url_branch {
        (value.to_string(), "url".to_string())
    } else {
        (
            choose_detected_branch(&normalized_repo_url)?,
            "detected".to_string(),
        )
    };

    if !remote_branch_exists(&normalized_repo_url, &selected_branch).map_err(|e| e.to_string())? {
        return Err(format!(
            "Remote branch '{}' was not found for {}.",
            selected_branch, normalized_repo_url
        ));
    }

    let has_root_flake = remote_branch_has_flake(&normalized_repo_url, &selected_branch)
        .map_err(|e| e.to_string())?;

    Ok(ProjectCreateProbe {
        suggested_name: derive_project_name_from_repo_url(&normalized_repo_url),
        normalized_repo_url,
        selected_branch,
        branch_source,
        has_root_flake,
        worker_image: current_default_sandbox_image(),
    })
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

// ── Page data loading ──

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
    let history = shepherd_runtime::get_shepherd_conversation(project_id).await?;
    let activity = shepherd_runtime::get_shepherd_activity(project_id).await?;

    Ok(ProjectPageState {
        projects,
        project,
        surface,
        threads: resolved_threads,
        history,
        activity,
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
