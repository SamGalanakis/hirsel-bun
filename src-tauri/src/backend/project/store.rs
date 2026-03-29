//! Project storage in global SQLite database

use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::focus::default_project_focus_html;
use super::types::{
    CreateProjectRequest, Project, ProjectFocusView, ProjectRetainedContext, UpdateProjectRequest,
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
    x REAL,
    y REAL,
    active_route_id INTEGER
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
"#;

const SCHEMA_VERSION_KEY: &str = "project_route_model_version";
const SCHEMA_VERSION: &str = "2";

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

/// Hard reset for project/route-owned domain tables.
///
/// This is intentionally destructive: route ownership changed and old layouts are
/// not supported.
async fn reset_project_domain(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    const RESET_SQL: &str = r#"
DROP TABLE IF EXISTS project_repos;
DROP TABLE IF EXISTS route_repos;
DROP TABLE IF EXISTS routes;
DROP TABLE IF EXISTS projects;
DROP TABLE IF EXISTS board_node_checked_by;
DROP TABLE IF EXISTS board_node_blocked_by;
DROP TABLE IF EXISTS board_nodes;
DROP TABLE IF EXISTS route_runtimes;
DROP TABLE IF EXISTS board_versions;
DROP TABLE IF EXISTS delivery_attempts;
DROP TABLE IF EXISTS deliveries;
DROP TABLE IF EXISTS worker_concerns;
DROP TABLE IF EXISTS worker_concern_reads;
DROP TABLE IF EXISTS project_focus_views;
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
    #[error("Route error: {0}")]
    Route(#[from] crate::backend::route::RouteError),
    #[error("Work tree error: {0}")]
    WorkTree(String),
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

        if req.repos.is_empty() {
            return Err(ProjectError::InvalidInput(
                "Project must include at least one repo".to_string(),
            ));
        }

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE name = ?)")
                .bind(&req.name)
                .fetch_one(pool)
                .await?;

        if exists {
            return Err(ProjectError::AlreadyExists(req.name.clone()));
        }

        let now = utc_now();
        let result = sqlx::query(
            "INSERT INTO projects (name, created_at, updated_at, description, x, y, active_route_id)
             VALUES (?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind(&req.name)
        .bind(&now)
        .bind(&now)
        .bind(&req.description)
        .bind(req.x)
        .bind(req.y)
        .execute(pool)
        .await?;

        self.get_project(result.last_insert_rowid()).await
    }

    /// Select the active route for a project.
    pub async fn set_active_route_id(&self, project_id: i64, route_id: i64) -> ProjectResult<()> {
        let pool = self.pool().await;
        let _ = self.get_project(project_id).await?;
        sqlx::query("UPDATE projects SET active_route_id = ?, updated_at = ? WHERE id = ?")
            .bind(route_id)
            .bind(utc_now())
            .bind(project_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Get a project by ID
    pub async fn get_project(&self, id: i64) -> ProjectResult<Project> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at, description, icon, x, y, active_route_id
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
            "SELECT id, name, created_at, updated_at, description, icon, x, y, active_route_id
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
            "SELECT id, name, created_at, updated_at, description, icon, x, y, active_route_id
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

    /// Replace only the project's description.
    pub async fn set_project_description(
        &self,
        id: i64,
        description: Option<&str>,
    ) -> ProjectResult<Project> {
        let pool = self.pool().await;

        let _ = self.get_project(id).await?;

        sqlx::query("UPDATE projects SET description = ?, updated_at = ? WHERE id = ?")
            .bind(description)
            .bind(utc_now())
            .bind(id)
            .execute(pool)
            .await?;

        self.get_project(id).await
    }

    /// Delete a project and all associated data
    pub async fn delete_project(&self, id: i64) -> ProjectResult<()> {
        let pool = self.pool().await;

        let _ = self.get_project(id).await?;

        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;

        // Best-effort cleanup for route-scoped/project-scoped tables
        let _ = sqlx::query("DELETE FROM board_node_checked_by WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM board_node_blocked_by WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM board_nodes WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM route_runtimes WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM route_repos WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM routes WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query(
            "DELETE FROM worker_concern_reads WHERE concern_id IN (SELECT id FROM worker_concerns WHERE project_id = ?)",
        )
        .bind(id)
        .execute(pool)
        .await;
        let _ = sqlx::query("DELETE FROM worker_concerns WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM project_focus_views WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM project_retained_contexts WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;

        if let Ok(shepherd_store) = crate::backend::shepherd_chat::ShepherdChatStore::open().await {
            let _ = shepherd_store.delete_project_messages(id).await;
        }
        if let Ok(thread_store) =
            crate::backend::shepherd_threads::ShepherdThreadStore::open().await
        {
            let _ = thread_store.delete_project_threads(id).await;
        }

        let project_dir = crate::backend::config::hirsel_dir()
            .join("projects")
            .join(id.to_string());
        if project_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&project_dir) {
                tracing::warn!(
                    "Failed to delete project directory {:?}: {}",
                    project_dir,
                    e
                );
            }
        }

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

    fn row_to_project(row: &sqlx::sqlite::SqliteRow) -> Project {
        Project {
            id: row.get("id"),
            name: row.get("name"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            description: row.get("description"),
            icon: row.get("icon"),
            x: row.get("x"),
            y: row.get("y"),
            active_route_id: row.get("active_route_id"),
        }
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
