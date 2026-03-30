//! Project storage in global SQLite database

use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::focus::default_project_focus_html;
use super::types::{
    CreateProjectRequest, Project, ProjectFocusView, ProjectPreparationStep,
    ProjectRetainedContext, ProjectRuntimePreparation, UpdateProjectRequest,
};
use crate::backend::db::{global_pool, utc_now};

/// Schema for projects table.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    description TEXT,
    starting_point_json TEXT NOT NULL,
    sandbox_image TEXT,
    x REAL,
    y REAL
);

CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name);

CREATE TABLE IF NOT EXISTS project_focus_views (
    project_id INTEGER PRIMARY KEY,
    html TEXT NOT NULL,
    source TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_retained_contexts (
    project_id INTEGER PRIMARY KEY,
    markdown TEXT NOT NULL,
    source TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_runtime_preparations (
    project_id INTEGER PRIMARY KEY,
    status TEXT NOT NULL,
    headline TEXT NOT NULL,
    detail TEXT,
    progress REAL NOT NULL,
    steps_json TEXT NOT NULL,
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

const SCHEMA_VERSION_KEY: &str = "project_thread_model_version";
const SCHEMA_VERSION: &str = "7";

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    SCHEMA_INIT
        .get_or_try_init(|| async {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS app_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            )
            .execute(pool)
            .await?;

            let current_version: Option<String> =
                sqlx::query_scalar("SELECT value FROM app_meta WHERE key = ?")
                    .bind(SCHEMA_VERSION_KEY)
                    .fetch_optional(pool)
                    .await?;

            if current_version.as_deref() != Some(SCHEMA_VERSION) {
                reset_project_domain(pool).await?;
                sqlx::query("INSERT OR REPLACE INTO app_meta (key, value) VALUES (?, ?)")
                    .bind(SCHEMA_VERSION_KEY)
                    .bind(SCHEMA_VERSION)
                    .execute(pool)
                    .await?;
            }

            sqlx::raw_sql(SCHEMA).execute(pool).await?;

            // Additive column migrations (safe to re-run)
            let has_icon: bool = sqlx::query_scalar(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('projects') WHERE name = 'icon'",
            )
            .fetch_one(pool)
            .await?;
            if !has_icon {
                sqlx::query("ALTER TABLE projects ADD COLUMN icon TEXT")
                    .execute(pool)
                    .await?;
            }

            Ok::<(), sqlx::Error>(())
        })
        .await?;
    Ok(())
}

/// Hard reset for legacy project-owned domain tables.
///
/// This is intentionally destructive: only the current project/thread model is
/// supported.
async fn reset_project_domain(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    const RESET_SQL: &str = r#"
DROP TABLE IF EXISTS projects;
DROP TABLE IF EXISTS project_focus_views;
DROP TABLE IF EXISTS project_retained_contexts;
DROP TABLE IF EXISTS project_runtime_preparations;
DROP TABLE IF EXISTS shepherd_threads;
DROP TABLE IF EXISTS shepherd_chat_messages;
DROP TABLE IF EXISTS shepherd_live_turns;
DROP TABLE IF EXISTS shepherd_scope_states;
DROP TABLE IF EXISTS shepherd_sessions;
DROP TABLE IF EXISTS project_repos;
DROP TABLE IF EXISTS route_repos;
DROP TABLE IF EXISTS routes;
DROP TABLE IF EXISTS board_node_checked_by;
DROP TABLE IF EXISTS board_node_blocked_by;
DROP TABLE IF EXISTS board_nodes;
DROP TABLE IF EXISTS route_runtimes;
DROP TABLE IF EXISTS board_versions;
DROP TABLE IF EXISTS delivery_attempts;
DROP TABLE IF EXISTS deliveries;
DROP TABLE IF EXISTS worker_concerns;
DROP TABLE IF EXISTS worker_concern_reads;
DROP TABLE IF EXISTS meta;
"#;

    sqlx::raw_sql(RESET_SQL).execute(pool).await?;
    Ok(())
}

/// Error type for project operations
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
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

/// Project store backed by global SQLite database
pub struct ProjectStore;

impl ProjectStore {
    /// Open the global project store
    pub async fn open() -> ProjectResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    /// Get the pool
    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    /// Insert a bare project row.
    pub async fn create_project_record(
        &self,
        req: &CreateProjectRequest,
    ) -> ProjectResult<Project> {
        let pool = self.pool().await;

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE name = ?)")
                .bind(&req.name)
                .fetch_one(pool)
                .await?;

        if exists {
            return Err(ProjectError::AlreadyExists(req.name.clone()));
        }

        let starting_point_json = serde_json::to_string(&req.starting_point)
            .map_err(|e| ProjectError::InvalidInput(format!("Invalid starting point: {}", e)))?;
        let now = utc_now();
        let result = sqlx::query(
            "INSERT INTO projects (name, created_at, updated_at, description, starting_point_json, sandbox_image, x, y)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&req.name)
        .bind(&now)
        .bind(&now)
        .bind(&req.description)
        .bind(&starting_point_json)
        .bind(&req.sandbox_image)
        .bind(req.x)
        .bind(req.y)
        .execute(pool)
        .await?;

        self.get_project(result.last_insert_rowid()).await
    }

    /// Get a project by ID
    pub async fn get_project(&self, id: i64) -> ProjectResult<Project> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at, description, icon, starting_point_json, sandbox_image, x, y
             FROM projects
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ProjectError::NotFound(id.to_string()))?;

        Ok(Self::row_to_project(&row))
    }

    /// Get a project by name
    pub async fn get_project_by_name(&self, name: &str) -> ProjectResult<Option<Project>> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at, description, icon, starting_point_json, sandbox_image, x, y
             FROM projects
             WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| Self::row_to_project(&r)))
    }

    /// List all projects
    pub async fn list_projects(&self) -> ProjectResult<Vec<Project>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            "SELECT id, name, created_at, updated_at, description, icon, starting_point_json, sandbox_image, x, y
             FROM projects
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_project).collect())
    }

    /// Update project metadata.
    pub async fn update_project(
        &self,
        id: i64,
        req: &UpdateProjectRequest,
    ) -> ProjectResult<Project> {
        let pool = self.pool().await;

        let _ = self.get_project(id).await?;

        let now = utc_now();

        let mut updates = vec!["updated_at = ?".to_string()];
        let mut bind_index = 2usize;

        if req.name.is_some() {
            updates.push(format!("name = ?{}", bind_index));
            bind_index += 1;
        }
        if req.description.is_some() {
            updates.push(format!("description = ?{}", bind_index));
            bind_index += 1;
        }
        if req.sandbox_image.is_some() {
            updates.push(format!("sandbox_image = ?{}", bind_index));
            bind_index += 1;
        }
        if req.x.is_some() {
            updates.push(format!("x = ?{}", bind_index));
            bind_index += 1;
        }
        if req.y.is_some() {
            updates.push(format!("y = ?{}", bind_index));
            bind_index += 1;
        }

        let sql = format!(
            "UPDATE projects SET {} WHERE id = ?{}",
            updates.join(", "),
            bind_index
        );

        let mut query = sqlx::query(&sql).bind(&now);

        if let Some(ref name) = req.name {
            query = query.bind(name);
        }
        if let Some(ref description) = req.description {
            query = query.bind(description);
        }
        if let Some(ref sandbox_image) = req.sandbox_image {
            query = query.bind(sandbox_image);
        }
        if let Some(x) = req.x {
            query = query.bind(x);
        }
        if let Some(y) = req.y {
            query = query.bind(y);
        }

        query = query.bind(id);
        query.execute(pool).await?;

        self.get_project(id).await
    }

    /// Delete a project and all associated data
    pub async fn delete_project(&self, id: i64) -> ProjectResult<()> {
        let pool = self.pool().await;

        let _ = self.get_project(id).await?;

        if let Ok(shepherd_store) = crate::backend::shepherd_chat::ShepherdChatStore::open().await {
            let _ = shepherd_store.delete_project_messages(id).await;
        }
        if let Ok(thread_store) =
            crate::backend::shepherd_threads::ShepherdThreadStore::open().await
        {
            let _ = thread_store.delete_project_threads(id).await;
        }

        let workspace_dir =
            crate::backend::config::workspace_dir(&crate::backend::workspace_name_for_project(id));
        if workspace_dir.exists() {
            if let Err(error) = std::fs::remove_dir_all(&workspace_dir) {
                tracing::warn!(%error, project_id = id, path = %workspace_dir.display(), "failed to delete project workspace directory");
            }
        }

        let _ = sqlx::query("DELETE FROM project_focus_views WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM project_retained_contexts WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM project_runtime_preparations WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await;

        Ok(())
    }

    pub async fn get_project_focus_view(&self, id: i64) -> ProjectResult<ProjectFocusView> {
        let pool = self.pool().await;
        let project = self.get_project(id).await?;

        let row = sqlx::query(
            "SELECT project_id, html, source, updated_at FROM project_focus_views WHERE project_id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(ProjectFocusView {
                project_id: row.get("project_id"),
                html: row.get("html"),
                source: row.get("source"),
                updated_at: row.get("updated_at"),
            })
        } else {
            let html = default_project_focus_html(&project.name);
            self.update_project_focus_view(id, &html, Some("placeholder"))
                .await
        }
    }

    pub async fn update_project_focus_view(
        &self,
        id: i64,
        html: &str,
        source: Option<&str>,
    ) -> ProjectResult<ProjectFocusView> {
        let pool = self.pool().await;
        let _ = self.get_project(id).await?;
        let now = utc_now();

        sqlx::query(
            "INSERT INTO project_focus_views (project_id, html, source, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(project_id) DO UPDATE SET
               html = excluded.html,
               source = excluded.source,
               updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(html)
        .bind(source)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(ProjectFocusView {
            project_id: id,
            html: html.to_string(),
            source: source.map(ToOwned::to_owned),
            updated_at: now,
        })
    }

    pub async fn get_project_retained_context(
        &self,
        id: i64,
    ) -> ProjectResult<ProjectRetainedContext> {
        let pool = self.pool().await;
        let project = self.get_project(id).await?;

        let row = sqlx::query(
            "SELECT project_id, markdown, source, updated_at
             FROM project_retained_contexts
             WHERE project_id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(ProjectRetainedContext {
                project_id: row.get("project_id"),
                markdown: row.get("markdown"),
                source: row.get("source"),
                updated_at: row.get("updated_at"),
            })
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
        let pool = self.pool().await;
        let _ = self.get_project(id).await?;
        let now = utc_now();

        sqlx::query(
            "INSERT INTO project_retained_contexts (project_id, markdown, source, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(project_id) DO UPDATE SET
               markdown = excluded.markdown,
               source = excluded.source,
               updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(markdown)
        .bind(source)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(ProjectRetainedContext {
            project_id: id,
            markdown: markdown.to_string(),
            source: source.map(ToOwned::to_owned),
            updated_at: now,
        })
    }

    /// Set the project icon URL (or clear it with None).
    pub async fn set_project_icon(&self, id: i64, icon: Option<&str>) -> ProjectResult<Project> {
        let pool = self.pool().await;
        let _ = self.get_project(id).await?;

        sqlx::query("UPDATE projects SET icon = ?, updated_at = ? WHERE id = ?")
            .bind(icon)
            .bind(utc_now())
            .bind(id)
            .execute(pool)
            .await?;

        self.get_project(id).await
    }

    pub async fn get_project_runtime_preparation(
        &self,
        id: i64,
    ) -> ProjectResult<Option<ProjectRuntimePreparation>> {
        let pool = self.pool().await;
        let _ = self.get_project(id).await?;

        let row = sqlx::query(
            "SELECT project_id, status, headline, detail, progress, steps_json, started_at, updated_at
             FROM project_runtime_preparations
             WHERE project_id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(Self::row_to_project_runtime_preparation)
            .transpose()
    }

    pub async fn save_project_runtime_preparation(
        &self,
        state: &ProjectRuntimePreparation,
    ) -> ProjectResult<ProjectRuntimePreparation> {
        let pool = self.pool().await;
        let _ = self.get_project(state.project_id).await?;
        let steps_json = serde_json::to_string(&state.steps).map_err(|error| {
            ProjectError::InvalidInput(format!("Invalid preparation steps: {}", error))
        })?;

        sqlx::query(
            "INSERT INTO project_runtime_preparations (
                project_id, status, headline, detail, progress, steps_json, started_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(project_id) DO UPDATE SET
                status = excluded.status,
                headline = excluded.headline,
                detail = excluded.detail,
                progress = excluded.progress,
                steps_json = excluded.steps_json,
                started_at = excluded.started_at,
                updated_at = excluded.updated_at",
        )
        .bind(state.project_id)
        .bind(&state.status)
        .bind(&state.headline)
        .bind(&state.detail)
        .bind(state.progress)
        .bind(&steps_json)
        .bind(&state.started_at)
        .bind(&state.updated_at)
        .execute(pool)
        .await?;

        Ok(state.clone())
    }

    fn row_to_project(row: &sqlx::sqlite::SqliteRow) -> Project {
        let starting_point_json: String = row.get("starting_point_json");
        let starting_point = serde_json::from_str(&starting_point_json)
            .unwrap_or(crate::backend::draft::StartingPoint::Greenfield);
        Project {
            id: row.get("id"),
            name: row.get("name"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            description: row.get("description"),
            icon: row.get("icon"),
            starting_point,
            sandbox_image: row.get("sandbox_image"),
            x: row.get("x"),
            y: row.get("y"),
        }
    }

    fn row_to_project_runtime_preparation(
        row: sqlx::sqlite::SqliteRow,
    ) -> ProjectResult<ProjectRuntimePreparation> {
        let steps_json: String = row.get("steps_json");
        let steps: Vec<ProjectPreparationStep> =
            serde_json::from_str(&steps_json).map_err(|error| {
                ProjectError::InvalidInput(format!("Invalid preparation steps JSON: {}", error))
            })?;
        Ok(ProjectRuntimePreparation {
            project_id: row.get("project_id"),
            status: row.get("status"),
            headline: row.get("headline"),
            detail: row.get("detail"),
            progress: row.get("progress"),
            steps,
            started_at: row.get("started_at"),
            updated_at: row.get("updated_at"),
        })
    }
}

fn default_project_retained_context_markdown(project_name: &str) -> String {
    format!(
        "# Retained Context\n\nProject: {}\n\nKeep durable findings, constraints, and decisions here.\n",
        project_name
    )
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
