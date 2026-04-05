//! Project storage in the global SurrealDB database.

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use super::types::{
    CreateProjectRequest, Project, ProjectPreparationStep, ProjectRetainedContext,
    ProjectRuntimePreparation, UpdateProjectRequest,
};
use crate::backend::db::{global_db, next_sequence, utc_now, DbClient};
use crate::backend::live_updates::{self, LiveUpdateKind};

const PROJECT_TABLE: &str = "project";
const PROJECT_RETAINED_CONTEXT_TABLE: &str = "project_retained_context";
const PROJECT_RUNTIME_PREPARATION_TABLE: &str = "project_runtime_preparation";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ProjectRecord {
    project_id: i64,
    name: String,
    name_lower: String,
    #[serde(default)]
    workspace_key: Option<String>,
    created_at: String,
    updated_at: String,
    description: Option<String>,
    icon: Option<String>,
    starting_point: crate::backend::draft::StartingPoint,
    sandbox_image: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ProjectRetainedContextRecord {
    project_id: i64,
    markdown: String,
    source: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ProjectRuntimePreparationRecord {
    project_id: i64,
    status: String,
    headline: String,
    detail: Option<String>,
    progress: f64,
    steps: Vec<ProjectPreparationStep>,
    started_at: String,
    updated_at: String,
}

/// Error type for project operations.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Project not found: {0}")]
    NotFound(String),
    #[error("Project already exists: {0}")]
    AlreadyExists(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

pub type ProjectResult<T> = Result<T, ProjectError>;

/// Project store backed by the global SurrealDB database.
pub struct ProjectStore;

impl ProjectStore {
    /// Open the global project store.
    pub async fn open() -> ProjectResult<Self> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    /// Insert a bare project record.
    pub async fn create_project_record(
        &self,
        req: &CreateProjectRequest,
    ) -> ProjectResult<Project> {
        if self.get_project_by_name(&req.name).await?.is_some() {
            return Err(ProjectError::AlreadyExists(req.name.clone()));
        }

        let db = self.db().await;
        let id = next_sequence("project").await?;
        let now = utc_now();
        let record = ProjectRecord {
            project_id: id,
            name: req.name.clone(),
            name_lower: normalize_text(&req.name),
            workspace_key: Some(new_workspace_key()),
            created_at: now.clone(),
            updated_at: now,
            description: req.description.clone(),
            icon: None,
            starting_point: req.starting_point.clone(),
            sandbox_image: req.sandbox_image.clone(),
            x: req.x,
            y: req.y,
        };

        let _: Option<ProjectRecord> = db
            .create((PROJECT_TABLE, id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    /// Get a project by ID.
    pub async fn get_project(&self, id: i64) -> ProjectResult<Project> {
        let record = self.load_project_record(id).await?;
        record
            .map(ProjectRecord::into_project)
            .ok_or_else(|| ProjectError::NotFound(id.to_string()))
    }

    /// Get a project by name.
    pub async fn get_project_by_name(&self, name: &str) -> ProjectResult<Option<Project>> {
        let db = self.db().await;
        let normalized = normalize_text(name);
        let mut result = db
            .query("SELECT * FROM project WHERE name_lower = $name LIMIT 1")
            .bind(("name", normalized))
            .await?;
        let record: Option<ProjectRecord> = result.take(0)?;
        match record {
            Some(record) => Ok(Some(
                self.hydrate_project_record(record).await?.into_project(),
            )),
            None => Ok(None),
        }
    }

    /// List all projects.
    pub async fn list_projects(&self) -> ProjectResult<Vec<Project>> {
        let mut projects = self.load_all_project_records().await?;
        projects.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(projects
            .into_iter()
            .map(ProjectRecord::into_project)
            .collect())
    }

    /// Update project metadata.
    pub async fn update_project(
        &self,
        id: i64,
        req: &UpdateProjectRequest,
    ) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record = self
            .load_project_record(id)
            .await?
            .ok_or_else(|| ProjectError::NotFound(id.to_string()))?;

        if let Some(name) = req.name.as_ref() {
            if name != &record.name {
                if let Some(existing) = self.get_project_by_name(name).await? {
                    if existing.id != id {
                        return Err(ProjectError::AlreadyExists(name.clone()));
                    }
                }
                record.name = name.clone();
                record.name_lower = normalize_text(name);
            }
        }
        if let Some(description) = req.description.as_ref() {
            record.description = Some(description.clone());
        }
        if let Some(sandbox_image) = req.sandbox_image.as_ref() {
            record.sandbox_image = Some(sandbox_image.clone());
        }
        if let Some(x) = req.x {
            record.x = Some(x);
        }
        if let Some(y) = req.y {
            record.y = Some(y);
        }

        record.updated_at = utc_now();
        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    /// Delete a project and all associated data.
    pub async fn delete_project(&self, id: i64) -> ProjectResult<()> {
        let db = self.db().await;
        let project = self.get_project(id).await?;

        if let Ok(shepherd_store) = crate::backend::shepherd_chat::ShepherdChatStore::open().await {
            let _ = shepherd_store.delete_project_messages(id).await;
        }
        if let Ok(thread_store) =
            crate::backend::shepherd_threads::ShepherdThreadStore::open().await
        {
            let _ = thread_store.delete_project_threads(id).await;
        }

        for workspace_dir in [
            crate::backend::config::workspace_dir(&crate::backend::workspace_name_for_project(
                &project,
            )),
            crate::backend::config::workspace_dir(
                &crate::backend::legacy_workspace_name_for_project_id(id),
            ),
        ] {
            if workspace_dir.exists() {
                if let Err(error) = std::fs::remove_dir_all(&workspace_dir) {
                    tracing::warn!(%error, project_id = id, path = %workspace_dir.display(), "failed to delete project workspace directory");
                }
            }
        }

        let _: Option<ProjectRetainedContextRecord> =
            db.delete((PROJECT_RETAINED_CONTEXT_TABLE, id)).await?;
        let _: Option<ProjectRuntimePreparationRecord> =
            db.delete((PROJECT_RUNTIME_PREPARATION_TABLE, id)).await?;
        let _: Option<ProjectRecord> = db.delete((PROJECT_TABLE, id)).await?;
        live_updates::publish_project(id, LiveUpdateKind::ProjectChanged);

        Ok(())
    }

    pub async fn get_project_retained_context(
        &self,
        id: i64,
    ) -> ProjectResult<ProjectRetainedContext> {
        let db = self.db().await;
        let project = self.get_project(id).await?;

        let record: Option<ProjectRetainedContextRecord> =
            db.select((PROJECT_RETAINED_CONTEXT_TABLE, id)).await?;

        if let Some(record) = record {
            Ok(record.into_retained_context())
        } else {
            let markdown = default_project_retained_context_markdown(&project.name);
            self.update_project_retained_context(id, &markdown, Some("seed"))
                .await
        }
    }

    pub async fn update_project_retained_context(
        &self,
        id: i64,
        markdown: &str,
        source: Option<&str>,
    ) -> ProjectResult<ProjectRetainedContext> {
        let db = self.db().await;
        let _ = self.get_project(id).await?;
        let record = ProjectRetainedContextRecord {
            project_id: id,
            markdown: markdown.to_string(),
            source: source.map(ToOwned::to_owned),
            updated_at: utc_now(),
        };

        let _: Option<ProjectRetainedContextRecord> = db
            .upsert((PROJECT_RETAINED_CONTEXT_TABLE, id))
            .content(record.clone())
            .await?;

        Ok(record.into_retained_context())
    }

    /// Set the project icon URL, or clear it with `None`.
    pub async fn set_project_icon(&self, id: i64, icon: Option<&str>) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record = self
            .load_project_record(id)
            .await?
            .ok_or_else(|| ProjectError::NotFound(id.to_string()))?;

        record.icon = icon.map(ToOwned::to_owned);
        record.updated_at = utc_now();

        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    pub async fn get_project_runtime_preparation(
        &self,
        id: i64,
    ) -> ProjectResult<Option<ProjectRuntimePreparation>> {
        let db = self.db().await;
        let _ = self.get_project(id).await?;

        let record: Option<ProjectRuntimePreparationRecord> =
            db.select((PROJECT_RUNTIME_PREPARATION_TABLE, id)).await?;
        Ok(record.map(ProjectRuntimePreparationRecord::into_runtime_preparation))
    }

    pub async fn save_project_runtime_preparation(
        &self,
        state: &ProjectRuntimePreparation,
    ) -> ProjectResult<ProjectRuntimePreparation> {
        let db = self.db().await;
        let _ = self.get_project(state.project_id).await?;
        let record = ProjectRuntimePreparationRecord {
            project_id: state.project_id,
            status: state.status.clone(),
            headline: state.headline.clone(),
            detail: state.detail.clone(),
            progress: state.progress,
            steps: state.steps.clone(),
            started_at: state.started_at.clone(),
            updated_at: state.updated_at.clone(),
        };

        let _: Option<ProjectRuntimePreparationRecord> = db
            .upsert((PROJECT_RUNTIME_PREPARATION_TABLE, state.project_id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(state.project_id, LiveUpdateKind::ProjectPreparationChanged);

        Ok(record.into_runtime_preparation())
    }

    async fn load_project_record(&self, id: i64) -> ProjectResult<Option<ProjectRecord>> {
        let db = self.db().await;
        let record: Option<ProjectRecord> = db.select((PROJECT_TABLE, id)).await?;
        match record {
            Some(record) => Ok(Some(self.hydrate_project_record(record).await?)),
            None => Ok(None),
        }
    }

    async fn load_all_project_records(&self) -> ProjectResult<Vec<ProjectRecord>> {
        let db = self.db().await;
        let records: Vec<ProjectRecord> = db.select(PROJECT_TABLE).await?;
        let mut hydrated = Vec::with_capacity(records.len());
        for record in records {
            hydrated.push(self.hydrate_project_record(record).await?);
        }
        Ok(hydrated)
    }

    async fn hydrate_project_record(
        &self,
        mut record: ProjectRecord,
    ) -> ProjectResult<ProjectRecord> {
        if record
            .workspace_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Ok(record);
        }

        record.workspace_key = Some(new_workspace_key());
        let db = self.db().await;
        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, record.project_id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(record.project_id, LiveUpdateKind::ProjectChanged);
        Ok(record)
    }
}

impl ProjectRecord {
    fn into_project(self) -> Project {
        Project {
            id: self.project_id,
            name: self.name,
            workspace_key: self
                .workspace_key
                .unwrap_or_else(|| format!("legacy-{}", self.project_id)),
            created_at: self.created_at,
            updated_at: self.updated_at,
            description: self.description,
            icon: self.icon,
            starting_point: self.starting_point,
            sandbox_image: self.sandbox_image,
            x: self.x,
            y: self.y,
        }
    }
}

impl ProjectRetainedContextRecord {
    fn into_retained_context(self) -> ProjectRetainedContext {
        ProjectRetainedContext {
            project_id: self.project_id,
            markdown: self.markdown,
            updated_at: self.updated_at,
            source: self.source,
        }
    }
}

impl ProjectRuntimePreparationRecord {
    fn into_runtime_preparation(self) -> ProjectRuntimePreparation {
        ProjectRuntimePreparation {
            project_id: self.project_id,
            status: self.status,
            headline: self.headline,
            detail: self.detail,
            progress: self.progress,
            steps: self.steps,
            started_at: self.started_at,
            updated_at: self.updated_at,
        }
    }
}

fn normalize_text(value: &str) -> String {
    value.to_lowercase()
}

fn new_workspace_key() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    raw[..12].to_string()
}

fn default_project_retained_context_markdown(project_name: &str) -> String {
    format!(
        "# Retained Context\n\nProject: {}\n\nKeep durable findings, constraints, and decisions here.\n",
        project_name
    )
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now.
}
