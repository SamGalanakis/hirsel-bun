use crate::backend::documents;
use crate::backend::project::{
    CreateProjectRequest, Project, ProjectStore, ProjectSurfaceSnapshot, UpdateProjectRequest,
};

#[tracing::instrument]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store.list_projects().await.map_err(|e| e.to_string())
}

#[tracing::instrument]
pub async fn get_project_surface(project_id: i64) -> Result<ProjectSurfaceSnapshot, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    let _ = store
        .get_project(project_id)
        .await
        .map_err(|e| e.to_string())?;
    let canvas = documents::get_canvas_document(project_id).await?;
    Ok(ProjectSurfaceSnapshot { canvas })
}

#[tracing::instrument]
pub async fn create_project(name: String) -> Result<Project, String> {
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

    let req = CreateProjectRequest { name: project_name };
    let store2 = ProjectStore::open().await.map_err(|e| e.to_string())?;
    let project = store2
        .create_project_record(&req)
        .await
        .map_err(|e| e.to_string())?;

    // Seed retained context
    let _ = store2.get_project_retained_context(project.id).await;

    store2
        .get_project(project.id)
        .await
        .map_err(|e| format!("failed to reload project: {}", e))
}

#[tracing::instrument]
pub async fn update_project_settings(project_id: i64, name: String) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store
        .update_project(project_id, &UpdateProjectRequest { name: Some(name) })
        .await
        .map_err(|e| e.to_string())
}
