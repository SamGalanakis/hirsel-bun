//! Project storage in the global SurrealDB database.

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use super::types::{
    CreateProjectRequest, Project, ProjectRetainedContext, ProjectWorkspaceEntry,
    UpdateProjectRequest,
};
use crate::backend::db::{global_db, next_sequence, utc_now, DbClient};
use crate::backend::live_updates::{self, LiveUpdateKind};

const PROJECT_TABLE: &str = "project";
const PROJECT_RETAINED_CONTEXT_TABLE: &str = "project_retained_context";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ProjectRecord {
    project_id: i64,
    name: String,
    name_lower: String,
    created_at: String,
    updated_at: String,
    icon: Option<String>,
    #[serde(default)]
    workspaces: Vec<ProjectWorkspaceEntry>,
    #[serde(default)]
    shepherd_cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ProjectRetainedContextRecord {
    project_id: i64,
    markdown: String,
    source: Option<String>,
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
    pub async fn open() -> ProjectResult<Self> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

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
            created_at: now.clone(),
            updated_at: now,
            icon: None,
            workspaces: Vec::new(),
            shepherd_cwd: None,
        };

        let _: Option<ProjectRecord> = db
            .create((PROJECT_TABLE, id))
            .content(record.clone())
            .await?;

        // Seed the canvas document.
        let _ = db
            .query(
                "UPSERT type::record('project_canvas', $pid) MERGE {
                    html: '<hirsel-callout title=\"New Project\" tone=\"info\">Use the shepherd to explore your project. The Librarian will keep this canvas updated as you work.</hirsel-callout>',
                    source: 'system',
                }",
            )
            .bind(("pid", id))
            .await;

        // Seed the knowledge graph index document as markdown.
        let _ = db
            .query(
                "UPSERT type::record('kg_node', [$pid, 'document', 'index']) MERGE {
                    project_id: $pid,
                    kind: 'document',
                    node_id: 'index',
                    label: 'Project Index',
                    content: '# Project Index\n\nThis index is maintained by the librarian. It maps the project knowledge graph.\n\n## Components\n\n(none yet)\n\n## Domain Entities\n\n(none yet)\n\n## Conventions\n\n(none yet)\n\n## Decisions\n\n(none yet)\n\n## Facts\n\n(none yet)\n\n## Goals\n\n(none yet)',
                    subtype: 'markdown',
                    source: 'system',
                    tags: ['index'],
                    metadata: {},
                }",
            )
            .bind(("pid", id))
            .await;

        live_updates::publish_project(id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    pub async fn get_project(&self, id: i64) -> ProjectResult<Project> {
        let record: Option<ProjectRecord> = self.db().await.select((PROJECT_TABLE, id)).await?;
        record
            .map(ProjectRecord::into_project)
            .ok_or_else(|| ProjectError::NotFound(id.to_string()))
    }

    pub async fn get_project_by_name(&self, name: &str) -> ProjectResult<Option<Project>> {
        let db = self.db().await;
        let normalized = normalize_text(name);
        let mut result = db
            .query("SELECT * FROM project WHERE name_lower = $name LIMIT 1")
            .bind(("name", normalized))
            .await?;
        let record: Option<ProjectRecord> = result.take(0)?;
        Ok(record.map(ProjectRecord::into_project))
    }

    pub async fn list_projects(&self) -> ProjectResult<Vec<Project>> {
        let db = self.db().await;
        let mut records: Vec<ProjectRecord> = db.select(PROJECT_TABLE).await?;
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(records
            .into_iter()
            .map(ProjectRecord::into_project)
            .collect())
    }

    pub async fn update_project(
        &self,
        id: i64,
        req: &UpdateProjectRequest,
    ) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record: ProjectRecord = db
            .select((PROJECT_TABLE, id))
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
        record.updated_at = utc_now();
        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    /// Add a workspace to a project.
    pub async fn add_workspace(
        &self,
        project_id: i64,
        workspace: ProjectWorkspaceEntry,
    ) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record: ProjectRecord = db
            .select((PROJECT_TABLE, project_id))
            .await?
            .ok_or_else(|| ProjectError::NotFound(project_id.to_string()))?;

        record.workspaces.push(workspace);
        record.updated_at = utc_now();
        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, project_id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(project_id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    /// Remove a workspace from a project by workspace ID.
    pub async fn remove_workspace(
        &self,
        project_id: i64,
        workspace_id: &str,
    ) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record: ProjectRecord = db
            .select((PROJECT_TABLE, project_id))
            .await?
            .ok_or_else(|| ProjectError::NotFound(project_id.to_string()))?;

        record.workspaces.retain(|w| w.id != workspace_id);
        record.updated_at = utc_now();
        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, project_id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(project_id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    /// Insert or replace a workspace on a project by workspace ID.
    pub async fn upsert_workspace(
        &self,
        project_id: i64,
        workspace: ProjectWorkspaceEntry,
    ) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record: ProjectRecord = db
            .select((PROJECT_TABLE, project_id))
            .await?
            .ok_or_else(|| ProjectError::NotFound(project_id.to_string()))?;

        if let Some(existing) = record
            .workspaces
            .iter_mut()
            .find(|entry| entry.id == workspace.id)
        {
            *existing = workspace;
        } else {
            record.workspaces.push(workspace);
        }
        record.updated_at = utc_now();
        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, project_id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(project_id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    /// Update the shepherd's current working directory.
    pub async fn update_shepherd_cwd(
        &self,
        project_id: i64,
        cwd: Option<&str>,
    ) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record: ProjectRecord = db
            .select((PROJECT_TABLE, project_id))
            .await?
            .ok_or_else(|| ProjectError::NotFound(project_id.to_string()))?;

        record.shepherd_cwd = cwd.map(ToOwned::to_owned);
        record.updated_at = utc_now();
        let _: Option<ProjectRecord> = db
            .upsert((PROJECT_TABLE, project_id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(project_id, LiveUpdateKind::ProjectChanged);
        Ok(record.into_project())
    }

    pub async fn delete_project(&self, id: i64) -> ProjectResult<()> {
        let db = self.db().await;
        let _ = self.get_project(id).await?;

        if let Ok(shepherd_store) = crate::backend::shepherd_chat::ShepherdChatStore::open().await {
            let _ = shepherd_store.delete_project_messages(id).await;
        }
        if let Ok(thread_store) =
            crate::backend::shepherd_threads::ShepherdThreadStore::open().await
        {
            let _ = thread_store.delete_project_threads(id).await;
        }

        // Clean up knowledge graph and canvas data
        let _ = db
            .query("DELETE FROM kg_edge WHERE `in`[0] = $pid AND out[0] = $pid; DELETE FROM kg_node WHERE id[0] = $pid; DELETE type::record('project_canvas', $pid);")
            .bind(("pid", id))
            .await;

        // Clean up scope sessions
        let _ = db
            .query("DELETE FROM shepherd_scope_state WHERE project_id = $pid; DELETE FROM shepherd_session WHERE project_id = $pid; DELETE FROM shepherd_live_turn WHERE project_id = $pid; DELETE FROM librarian_job WHERE project_id = $pid; DELETE FROM librarian_event WHERE project_id = $pid; DELETE FROM project_recent_focus WHERE project_id = $pid;")
            .bind(("pid", id))
            .await;

        let _: Option<ProjectRetainedContextRecord> =
            db.delete((PROJECT_RETAINED_CONTEXT_TABLE, id)).await?;
        let _: Option<ProjectRecord> = db.delete((PROJECT_TABLE, id)).await?;

        tracing::info!(project_id = id, "project deleted with all associated data");
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

    pub async fn set_project_icon(&self, id: i64, icon: Option<&str>) -> ProjectResult<Project> {
        let db = self.db().await;
        let mut record: ProjectRecord = db
            .select((PROJECT_TABLE, id))
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
}

impl ProjectRecord {
    fn into_project(self) -> Project {
        Project {
            id: self.project_id,
            name: self.name,
            created_at: self.created_at,
            updated_at: self.updated_at,
            icon: self.icon,
            workspaces: self.workspaces,
            shepherd_cwd: self.shepherd_cwd,
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

fn normalize_text(value: &str) -> String {
    value.to_lowercase()
}

fn default_project_retained_context_markdown(project_name: &str) -> String {
    format!(
        "# Retained Context\n\nProject: {}\n\nKeep durable findings, constraints, and decisions here.\n",
        project_name
    )
}
