use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::backend::draft::StartingPoint;
use crate::backend::git::{inspect_remote_branches, parse_github_url};
use crate::backend::live_updates;
use crate::backend::{app, shepherd_runtime, ProjectStore};

use super::common::{
    extract_latest_plan, plan_progress_from_messages, to_api_activity, to_api_message,
    to_api_project, to_api_project_preparation, to_api_thread, ApiChatMessage, ApiProject,
    ApiScopeActivity, ApiThreadDetail, ApiThreadSummary, ApiWorkspaceSnapshot,
};

#[derive(Serialize)]
pub struct ApiProjectCreateProbe {
    normalized_repo_url: String,
    suggested_name: String,
    selected_branch: String,
    branch_source: String,
    worker_image: String,
}

#[derive(Serialize)]
pub struct ApiProjectSurface {
    canvas_node_id: Option<String>,
    canvas_label: Option<String>,
    canvas_html: Option<String>,
    canvas_source: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceSnapshotQuery {
    thread_id: Option<String>,
    librarian: Option<bool>,
}

struct ProjectCreateProbe {
    normalized_repo_url: String,
    suggested_name: String,
    selected_branch: String,
    branch_source: String,
    worker_image: String,
}

fn current_default_sandbox_image() -> String {
    crate::backend::config::Config::load()
        .map(|(config, _)| config.sandbox.image)
        .unwrap_or_else(|_| crate::backend::sandbox::SandboxConfig::default().image)
}

fn effective_project_worker_image(project_image: Option<&str>) -> String {
    project_image
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(current_default_sandbox_image)
}

fn normalize_project_sandbox_image(raw: Option<&str>) -> Result<Option<String>, String> {
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

fn probe_project_create(
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
    let inspection =
        inspect_remote_branches(&normalized_repo_url).map_err(|error| error.to_string())?;
    let (selected_branch, branch_source) = if let Some(value) = explicit_branch {
        (value.to_string(), "explicit".to_string())
    } else if let Some(value) = url_branch {
        (value.to_string(), "url".to_string())
    } else {
        (
            inspection
                .default_branch
                .clone()
                .or_else(|| {
                    inspection
                        .branches
                        .iter()
                        .find(|branch| branch.as_str() == "main")
                        .or_else(|| {
                            inspection
                                .branches
                                .iter()
                                .find(|branch| branch.as_str() == "master")
                        })
                        .cloned()
                        .or_else(|| inspection.branches.first().cloned())
                })
                .ok_or_else(|| {
                    "No visible remote branches were found for this repository.".to_string()
                })?,
            "detected".to_string(),
        )
    };

    if !inspection
        .branches
        .iter()
        .any(|branch| branch == &selected_branch)
    {
        return Err(format!(
            "Remote branch '{}' was not found for {}.",
            selected_branch, normalized_repo_url
        ));
    }

    Ok(ProjectCreateProbe {
        suggested_name: derive_project_name_from_repo_url(&normalized_repo_url),
        normalized_repo_url,
        selected_branch,
        branch_source,
        worker_image: current_default_sandbox_image(),
    })
}

async fn persist_project_and_start_runtime_preparation(
    name: String,
    repo_url: String,
    branch: Option<String>,
    sandbox_image: Option<String>,
) -> Result<crate::backend::Project, (StatusCode, String)> {
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
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    crate::backend::start_project_runtime_preparation(project.id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(project)
}

fn to_api_project_create_probe(probe: ProjectCreateProbe) -> ApiProjectCreateProbe {
    ApiProjectCreateProbe {
        normalized_repo_url: probe.normalized_repo_url,
        suggested_name: probe.suggested_name,
        selected_branch: probe.selected_branch,
        branch_source: probe.branch_source,
        worker_image: probe.worker_image,
    }
}

async fn load_project(project_id: i64) -> Result<crate::backend::Project, (StatusCode, String)> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    store
        .get_project(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

#[derive(Deserialize)]
pub struct ProjectCreateProbeQuery {
    repo_url: String,
    branch: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct ChatSendBody {
    content: String,
}

#[derive(Deserialize)]
pub struct CreateProjectBody {
    name: String,
    repo_url: String,
    branch: Option<String>,
    sandbox_image: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveProjectSettingsBody {
    name: String,
    description: Option<String>,
    sandbox_image: Option<String>,
}

pub async fn list_projects() -> Result<impl IntoResponse, (StatusCode, String)> {
    let projects = app::list_projects()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let out: Vec<ApiProject> = projects.iter().map(to_api_project).collect();
    Ok(Json(out))
}

pub async fn get_project(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let project = load_project(project_id).await?;
    Ok(Json(to_api_project(&project)))
}

pub async fn project_events(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let _ = load_project(project_id).await?;
    let mut receiver = live_updates::subscribe();

    let stream = async_stream::stream! {
        yield Ok::<Event, Infallible>(
            Event::default()
                .event("ready")
                .data(format!(r#"{{"projectId":{project_id}}}"#)),
        );

        loop {
            match receiver.recv().await {
                Ok(event) if event.project_id == project_id => {
                    match serde_json::to_string(&event) {
                        Ok(data) => yield Ok(Event::default().event("update").data(data)),
                        Err(error) => tracing::warn!(%error, project_id, "failed to serialize live update event"),
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(project_id, skipped, "live update stream lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

pub async fn probe_project_create_api(
    Query(query): Query<ProjectCreateProbeQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let probe = probe_project_create(&query.repo_url, query.branch.as_deref())
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(to_api_project_create_probe(probe)))
}

pub async fn get_project_preparation(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let project = load_project(project_id).await?;
    let state = crate::backend::get_project_runtime_preparation(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "project runtime preparation has not started".to_string(),
            )
        })?;
    Ok(Json(to_api_project_preparation(
        &project,
        &state,
        effective_project_worker_image(project.sandbox_image.as_deref()),
    )))
}

pub async fn start_project_preparation(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let project = load_project(project_id).await?;
    let state = crate::backend::start_project_runtime_preparation(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(to_api_project_preparation(
        &project,
        &state,
        effective_project_worker_image(project.sandbox_image.as_deref()),
    )))
}

pub async fn retry_project_preparation(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let project = load_project(project_id).await?;
    let state = crate::backend::retry_project_runtime_preparation(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(to_api_project_preparation(
        &project,
        &state,
        effective_project_worker_image(project.sandbox_image.as_deref()),
    )))
}

pub async fn get_project_activity(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let activity = shepherd_runtime::get_shepherd_activity(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json::<ApiScopeActivity>(to_api_activity(&activity)))
}

pub async fn get_project_history(
    Path(project_id): Path<i64>,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let history = shepherd_runtime::get_shepherd_history(
        shepherd_runtime::ShepherdScope::Shepherd {
            project_id,
            workspace_path: None,
            focus: None,
        },
        limit,
    )
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let out: Vec<ApiChatMessage> = history.iter().map(to_api_message).collect();
    Ok(Json(out))
}

pub async fn get_project_surface(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let surface = app::get_project_surface(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let out = ApiProjectSurface {
        canvas_node_id: surface.canvas.as_ref().map(|doc| doc.node_id.clone()),
        canvas_label: surface.canvas.as_ref().map(|doc| doc.label.clone()),
        canvas_source: surface.canvas.as_ref().and_then(|doc| doc.source.clone()),
        canvas_html: surface.canvas.as_ref().map(|doc| doc.html.clone()),
    };
    Ok(Json(out))
}

pub async fn get_workspace_snapshot(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceSnapshotQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let project = load_project(project_id).await?;
    let project_activity = shepherd_runtime::get_shepherd_activity(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let project_history = shepherd_runtime::get_shepherd_history(
        shepherd_runtime::ShepherdScope::Shepherd {
            project_id,
            workspace_path: None,
            focus: None,
        },
        100,
    )
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let surface = app::get_project_surface(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let thread_store = crate::backend::ShepherdThreadStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let threads = shepherd_runtime::get_project_threads(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    let mut thread_summaries = Vec::with_capacity(threads.len());
    for thread in threads {
        let history =
            shepherd_runtime::get_thread_conversation(project_id, &thread.id, &thread.title, 64)
                .await
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        let activity = shepherd_runtime::get_thread_activity(project_id, &thread.id, &thread.title)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        thread_summaries.push(ApiThreadSummary {
            thread: to_api_thread(&thread),
            activity: to_api_activity(&activity),
            plan_progress: plan_progress_from_messages(&history),
        });
    }

    let thread_detail = if let Some(thread_id) = query
        .thread_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let thread = thread_store
            .get_thread(thread_id)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        if thread.project_id != project_id {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "thread {} does not belong to project {}",
                    thread_id, project_id
                ),
            ));
        }
        let history =
            shepherd_runtime::get_thread_conversation(project_id, &thread.id, &thread.title, 200)
                .await
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        let activity = shepherd_runtime::get_thread_activity(project_id, &thread.id, &thread.title)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        Some((
            ApiThreadDetail {
                thread: to_api_thread(&thread),
                activity: to_api_activity(&activity),
                plan: extract_latest_plan(&history),
            },
            history,
        ))
    } else {
        None
    };

    let librarian_activity =
        shepherd_runtime::get_scope_activity(shepherd_runtime::ShepherdScope::Librarian {
            project_id,
            workspace_path: None,
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let librarian_history = if query.librarian.unwrap_or(false) {
        shepherd_runtime::get_shepherd_history(
            shepherd_runtime::ShepherdScope::Librarian {
                project_id,
                workspace_path: None,
            },
            200,
        )
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?
    } else {
        Vec::new()
    };

    Ok(Json(ApiWorkspaceSnapshot {
        project: to_api_project(&project),
        project_activity: to_api_activity(&project_activity),
        project_history: project_history.iter().map(to_api_message).collect(),
        surface: ApiProjectSurface {
            canvas_node_id: surface.canvas.as_ref().map(|doc| doc.node_id.clone()),
            canvas_label: surface.canvas.as_ref().map(|doc| doc.label.clone()),
            canvas_source: surface.canvas.as_ref().and_then(|doc| doc.source.clone()),
            canvas_html: surface.canvas.as_ref().map(|doc| doc.html.clone()),
        },
        threads: thread_summaries,
        thread_detail: thread_detail.as_ref().map(|(detail, _)| detail.clone()),
        thread_history: thread_detail
            .as_ref()
            .map(|(_, history)| history.iter().map(to_api_message).collect())
            .unwrap_or_default(),
        librarian_activity: to_api_activity(&librarian_activity),
        librarian_history: librarian_history.iter().map(to_api_message).collect(),
    }))
}

pub async fn get_librarian_activity(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let activity =
        shepherd_runtime::get_scope_activity(shepherd_runtime::ShepherdScope::Librarian {
            project_id,
            workspace_path: None,
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json::<ApiScopeActivity>(to_api_activity(&activity)))
}

pub async fn get_librarian_history(
    Path(project_id): Path<i64>,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let history = shepherd_runtime::get_shepherd_history(
        shepherd_runtime::ShepherdScope::Librarian {
            project_id,
            workspace_path: None,
        },
        limit,
    )
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let out: Vec<ApiChatMessage> = history.iter().map(to_api_message).collect();
    Ok(Json(out))
}

pub async fn send_librarian_message(
    Path(project_id): Path<i64>,
    Json(body): Json<ChatSendBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let response = shepherd_runtime::send_scope_message(
        shepherd_runtime::ShepherdScope::Librarian {
            project_id,
            workspace_path: None,
        },
        Some(body.content),
        None,
        None,
    )
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(response))
}

pub async fn stop_librarian_chat(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    shepherd_runtime::interrupt_scope_turn(shepherd_runtime::ShepherdScope::Librarian {
        project_id,
        workspace_path: None,
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn trigger_knowledge_scan(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    crate::backend::librarian::trigger_scan(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn get_knowledge_graph(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::backend::db::global_db;

    let db = global_db().await;

    let mut result = db
        .query(
            "SELECT * FROM kg_node WHERE project_id = $project_id ORDER BY updated_at DESC LIMIT 200",
        )
        .bind(("project_id", project_id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("graph query failed: {}", e)))?;

    let nodes: Vec<serde_json::Value> = result.take(0).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read nodes: {}", e),
        )
    })?;

    let mut result = db
        .query("SELECT * FROM kg_edge WHERE project_id = $project_id LIMIT 1000")
        .bind(("project_id", project_id))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("edge query failed: {}", e),
            )
        })?;

    let edges: Vec<serde_json::Value> = result.take(0).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read edges: {}", e),
        )
    })?;

    Ok(Json(serde_json::json!({
        "nodes": nodes,
        "edges": edges,
    })))
}

pub async fn send_project_chat_message(
    Path(project_id): Path<i64>,
    Json(body): Json<ChatSendBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let chunks = crate::backend::skills::enrich_chat_message_chunks(project_id, body.content, true)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let response = shepherd_runtime::send_shepherd_message(project_id, None, Some(chunks))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(response))
}

pub async fn stop_project_chat(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let scope = shepherd_runtime::ShepherdScope::Shepherd {
        project_id,
        workspace_path: None,
        focus: None,
    };
    shepherd_runtime::interrupt_scope_turn(scope)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn create_project_api(
    Json(body): Json<CreateProjectBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let sandbox_image = normalize_project_sandbox_image(body.sandbox_image.as_deref())
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    let project = persist_project_and_start_runtime_preparation(
        body.name,
        body.repo_url,
        body.branch,
        sandbox_image,
    )
    .await?;

    Ok(Json(to_api_project(&project)))
}

pub async fn delete_project(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    store
        .delete_project(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn save_project_settings(
    Path(project_id): Path<i64>,
    Json(body): Json<SaveProjectSettingsBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let sandbox_image = normalize_project_sandbox_image(body.sandbox_image.as_deref())
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    let project =
        app::update_project_settings(project_id, body.name, body.description, sandbox_image)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    Ok(Json(to_api_project(&project)))
}
