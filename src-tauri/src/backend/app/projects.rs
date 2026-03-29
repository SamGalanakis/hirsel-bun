use crate::backend::draft::StartingPoint;
use crate::backend::project::{
    CreateProjectRequest, Project, ProjectStore, ProjectSurfaceSnapshot, UpdateProjectRequest,
};

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

        if let Some(icon_url) = detect_icon_from_starting_point(&req.starting_point) {
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

#[tracing::instrument]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store.list_projects().await.map_err(|e| e.to_string())
}

#[tracing::instrument]
pub async fn get_project_surface(project_id: i64) -> Result<ProjectSurfaceSnapshot, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    let focus_view = store
        .get_project_focus_view(project_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ProjectSurfaceSnapshot { focus_view })
}

#[tracing::instrument]
pub async fn create_project(
    name: String,
    starting_point: StartingPoint,
    sandbox_image: Option<String>,
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
        starting_point,
        description: None,
        sandbox_image,
        x,
        y,
    })
    .await
}

#[tracing::instrument]
pub async fn update_project_settings(
    project_id: i64,
    name: String,
    description: Option<String>,
    sandbox_image: Option<String>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store
        .update_project(
            project_id,
            &UpdateProjectRequest {
                name: Some(name),
                description,
                sandbox_image,
                x: None,
                y: None,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

fn detect_icon_from_starting_point(starting_point: &StartingPoint) -> Option<String> {
    match starting_point {
        StartingPoint::GitRepo { url, .. } => favicon_from_git_url(url),
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
            None
        }
        StartingPoint::Greenfield => None,
    }
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
