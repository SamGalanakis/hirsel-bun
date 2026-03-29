use crate::backend::delta::DeltaState;
use crate::backend::draft::StartingPoint;
use crate::backend::project::{
    CreateProjectRequest, Project, ProjectStore, ProjectSurfaceSnapshot, RouteSummary,
    UpdateProjectRequest,
};
use crate::backend::route::{CreateMainRouteRequest, CreateRouteRepoRequest, RouteStore};

pub struct ProjectLifecycleService;

impl ProjectLifecycleService {
    pub async fn create_project(req: &CreateProjectRequest) -> Result<Project, String> {
        let store = ProjectStore::open()
            .await
            .map_err(|e| format!("failed to open project store: {}", e))?;
        let project = store
            .create_project_record(req)
            .await
            .map_err(|e| e.to_string())?;

        let route_store = RouteStore::new(project.id)
            .await
            .map_err(|e| format!("failed to open route store: {}", e))?;
        let main_route = route_store
            .create_main_route_with_seed(&CreateMainRouteRequest {
                repos: req.repos.clone(),
                default_repo_index: req.default_repo_index,
                time_limit_minutes: None,
                human_in_the_loop: None,
                target_branch: None,
            })
            .await
            .map_err(|e| format!("failed to create main route: {}", e))?;

        store
            .set_active_route_id(project.id, main_route.id)
            .await
            .map_err(|e| format!("failed to select main route: {}", e))?;

        if let Some(icon_url) = detect_icon_from_repos(&req.repos) {
            let _ = store.set_project_icon(project.id, Some(&icon_url)).await;
        }

        let _ = store.get_project_focus_view(project.id).await;
        let _ = store.get_project_retained_context(project.id).await;

        store
            .get_project(project.id)
            .await
            .map_err(|e| format!("failed to reload project: {}", e))
    }
}

fn public_route_status(raw: Option<&str>) -> String {
    match raw.unwrap_or("idle") {
        "working" => "active".to_string(),
        "paused" | "idle" => "idle".to_string(),
        "failed" => "failed".to_string(),
        _ => "idle".to_string(),
    }
}

#[tracing::instrument]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store.list_projects().await.map_err(|e| e.to_string())
}

#[tracing::instrument]
pub async fn get_project_surface(project_id: i64) -> Result<ProjectSurfaceSnapshot, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| e.to_string())?;
    let focus_view = store
        .get_project_focus_view(project_id)
        .await
        .map_err(|e| e.to_string())?;

    let route_store = RouteStore::new(project_id)
        .await
        .map_err(|e| e.to_string())?;
    let routes = route_store.list_routes().await.map_err(|e| e.to_string())?;
    let mut summaries = Vec::with_capacity(routes.len());

    for route in routes {
        let delta = DeltaState::with_route(project_id, route.id);
        let project_run = delta.get_route_runtime().await.map_err(|e| e.to_string())?;
        summaries.push(RouteSummary {
            route_id: route.id,
            name: route.name,
            selected: project.active_route_id == Some(route.id),
            status: public_route_status(project_run.as_ref().map(|run| run.status.as_str())),
            updated_at: route.updated_at,
        });
    }

    Ok(ProjectSurfaceSnapshot {
        focus_view,
        routes: summaries,
    })
}

#[tracing::instrument]
pub async fn create_project(
    name: String,
    repos: Vec<CreateRouteRepoRequest>,
    default_repo_index: Option<usize>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;

    let mut project_name = name;
    if let Ok(Some(_)) = store.get_project_by_name(&project_name).await {
        let base_name = project_name.clone();
        let mut counter = 1;
        while store
            .get_project_by_name(&project_name)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            counter += 1;
            project_name = format!("{} ({})", base_name, counter);
        }
    }

    ProjectLifecycleService::create_project(&CreateProjectRequest {
        name: project_name,
        repos,
        default_repo_index,
        description: None,
        x,
        y,
    })
    .await
}

#[tracing::instrument]
pub async fn update_project_description(
    project_id: i64,
    description: Option<String>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store
        .set_project_description(project_id, description.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tracing::instrument]
pub async fn update_project_name(project_id: i64, name: String) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store
        .update_project(
            project_id,
            &UpdateProjectRequest {
                name: Some(name),
                description: None,
                x: None,
                y: None,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

fn detect_icon_from_repos(repos: &[CreateRouteRepoRequest]) -> Option<String> {
    for repo in repos {
        match &repo.starting_point {
            StartingPoint::GitRepo { url, .. } => {
                if let Some(icon) = favicon_from_git_url(url) {
                    return Some(icon);
                }
            }
            StartingPoint::LocalFolder { path } => {
                for candidate in &[
                    "favicon.ico",
                    "public/favicon.ico",
                    "static/favicon.ico",
                    "src/favicon.ico",
                    "assets/favicon.ico",
                ] {
                    let full = std::path::Path::new(path).join(candidate);
                    if full.exists() {
                        return Some(format!("file://{}", full.display()));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn favicon_from_git_url(url: &str) -> Option<String> {
    let domain = extract_domain(url)?;
    Some(format!(
        "https://www.google.com/s2/favicons?domain={}&sz=64",
        domain
    ))
}

fn extract_domain(url: &str) -> Option<&str> {
    if let Some(rest) = url.strip_prefix("git@") {
        return rest.split(':').next();
    }
    if url.contains("://") {
        let after_scheme = url.split("://").nth(1)?;
        let after_auth = if after_scheme.contains('@') {
            after_scheme.split('@').nth(1)?
        } else {
            after_scheme
        };
        return after_auth
            .split('/')
            .next()
            .map(|h| h.split(':').next().unwrap_or(h));
    }
    None
}
