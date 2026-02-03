//! Project storage in global SQLite database

use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use tokio::sync::OnceCell;

use super::types::{CreateProjectRequest, Project, UpdateProjectRequest};
use crate::core::db::{global_pool, utc_now};
use crate::core::draft::StartingPoint;

/// Schema for projects table
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    -- Normalized StartingPoint (no JSON blob)
    starting_point_type TEXT NOT NULL,  -- 'greenfield' | 'local_folder' | 'git_repo'
    starting_point_path TEXT,           -- for local_folder
    starting_point_url TEXT,            -- for git_repo
    starting_point_branch TEXT,         -- for git_repo

    -- Default configuration (inherited by runs)
    worker_scale TEXT,
    time_limit_minutes INTEGER,
    max_iterations INTEGER,
    human_in_the_loop INTEGER DEFAULT 1,

    -- Scribe/docs configuration
    docs_path TEXT DEFAULT 'docs',
    persist_docs_changes INTEGER DEFAULT 1,

    -- Metadata
    description TEXT,

    -- Delivery configuration
    target_branch TEXT,  -- Branch for PR/merge delivery (e.g., "staging", "main")

    -- Runner configuration
    runner TEXT,  -- Default runner for this project's runs

    -- Canvas position (for OneBoard portfolio view)
    x REAL,
    y REAL
);

CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name);
"#;

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    SCHEMA_INIT
        .get_or_try_init(|| async {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
            Ok::<(), sqlx::Error>(())
        })
        .await?;
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

    /// Create a new project
    pub async fn create_project(&self, req: &CreateProjectRequest) -> ProjectResult<Project> {
        let pool = self.pool().await;

        // Check if project with this name already exists
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE name = ?)")
                .bind(&req.name)
                .fetch_one(pool)
                .await?;

        if exists {
            return Err(ProjectError::AlreadyExists(req.name.clone()));
        }

        let now = utc_now();
        let (sp_type, sp_path, sp_url, sp_branch) =
            Self::normalize_starting_point(&req.starting_point);

        let human_in_the_loop = req.human_in_the_loop.unwrap_or(true);
        let docs_path = req.docs_path.as_deref().unwrap_or("docs");
        let persist_docs_changes = req.persist_docs_changes.unwrap_or(true);

        let result = sqlx::query(
            "INSERT INTO projects (
                name, created_at, updated_at,
                starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                worker_scale, time_limit_minutes, human_in_the_loop,
                docs_path, persist_docs_changes, description, target_branch, runner, x, y
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&req.name)
        .bind(&now)
        .bind(&now)
        .bind(&sp_type)
        .bind(&sp_path)
        .bind(&sp_url)
        .bind(&sp_branch)
        .bind(&req.worker_scale)
        .bind(req.time_limit_minutes)
        .bind(human_in_the_loop as i64)
        .bind(docs_path)
        .bind(persist_docs_changes as i64)
        .bind(&req.description)
        .bind(&req.target_branch)
        .bind(&req.runner)
        .bind(req.x)
        .bind(req.y)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        self.get_project(id).await
    }

    /// Get a project by ID
    pub async fn get_project(&self, id: i64) -> ProjectResult<Project> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at,
                    starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                    worker_scale, time_limit_minutes, human_in_the_loop,
                    docs_path, persist_docs_changes, description, target_branch, runner, x, y
             FROM projects
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ProjectError::NotFound(id.to_string()))?;

        Self::row_to_project(&row)
    }

    /// Get a project by name
    pub async fn get_project_by_name(&self, name: &str) -> ProjectResult<Option<Project>> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at,
                    starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                    worker_scale, time_limit_minutes, human_in_the_loop,
                    docs_path, persist_docs_changes, description, target_branch, runner, x, y
             FROM projects
             WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(Self::row_to_project(&r)?)),
            None => Ok(None),
        }
    }

    /// List all projects
    pub async fn list_projects(&self) -> ProjectResult<Vec<Project>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            "SELECT id, name, created_at, updated_at,
                    starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                    worker_scale, time_limit_minutes, human_in_the_loop,
                    docs_path, persist_docs_changes, description, target_branch, runner, x, y
             FROM projects
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        let mut projects = Vec::with_capacity(rows.len());
        for row in rows {
            projects.push(Self::row_to_project(&row)?);
        }

        Ok(projects)
    }

    /// Update a project
    pub async fn update_project(
        &self,
        id: i64,
        req: &UpdateProjectRequest,
    ) -> ProjectResult<Project> {
        let pool = self.pool().await;

        // Check if project exists
        let _ = self.get_project(id).await?;

        let now = utc_now();

        // Build dynamic UPDATE query based on what's provided
        let mut updates = vec!["updated_at = ?".to_string()];
        let mut bind_index = 2usize;

        if req.name.is_some() {
            updates.push(format!("name = ?{}", bind_index));
            bind_index += 1;
        }

        if req.starting_point.is_some() {
            updates.push(format!("starting_point_type = ?{}", bind_index));
            bind_index += 1;
            updates.push(format!("starting_point_path = ?{}", bind_index));
            bind_index += 1;
            updates.push(format!("starting_point_url = ?{}", bind_index));
            bind_index += 1;
            updates.push(format!("starting_point_branch = ?{}", bind_index));
            bind_index += 1;
        }

        if req.worker_scale.is_some() {
            updates.push(format!("worker_scale = ?{}", bind_index));
            bind_index += 1;
        }

        if req.time_limit_minutes.is_some() {
            updates.push(format!("time_limit_minutes = ?{}", bind_index));
            bind_index += 1;
        }

        if req.human_in_the_loop.is_some() {
            updates.push(format!("human_in_the_loop = ?{}", bind_index));
            bind_index += 1;
        }

        if req.docs_path.is_some() {
            updates.push(format!("docs_path = ?{}", bind_index));
            bind_index += 1;
        }

        if req.persist_docs_changes.is_some() {
            updates.push(format!("persist_docs_changes = ?{}", bind_index));
            bind_index += 1;
        }

        if req.description.is_some() {
            updates.push(format!("description = ?{}", bind_index));
            bind_index += 1;
        }

        if req.target_branch.is_some() {
            updates.push(format!("target_branch = ?{}", bind_index));
            bind_index += 1;
        }

        if req.runner.is_some() {
            updates.push(format!("runner = ?{}", bind_index));
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

        // Build query with bindings
        let mut query = sqlx::query(&sql).bind(&now);

        if let Some(ref name) = req.name {
            query = query.bind(name);
        }

        if let Some(ref sp) = req.starting_point {
            let (sp_type, sp_path, sp_url, sp_branch) = Self::normalize_starting_point(sp);
            query = query
                .bind(sp_type)
                .bind(sp_path)
                .bind(sp_url)
                .bind(sp_branch);
        }

        if let Some(ref ws) = req.worker_scale {
            query = query.bind(ws);
        }

        if let Some(tl) = req.time_limit_minutes {
            query = query.bind(tl);
        }

        if let Some(hitl) = req.human_in_the_loop {
            query = query.bind(hitl as i64);
        }

        if let Some(ref dp) = req.docs_path {
            query = query.bind(dp);
        }

        if let Some(pdc) = req.persist_docs_changes {
            query = query.bind(pdc as i64);
        }

        if let Some(ref desc) = req.description {
            query = query.bind(desc);
        }

        if let Some(ref tb) = req.target_branch {
            query = query.bind(tb);
        }

        if let Some(ref runner) = req.runner {
            query = query.bind(runner);
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

        // Check if project exists
        let _ = self.get_project(id).await?;

        // Delete from projects table
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;

        // Cascade delete delta-related tables (they're in the same DB)
        // These may not exist in older DBs, so ignore errors
        let _ = sqlx::query("DELETE FROM draft_nodes WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM live_nodes WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM project_runs WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM delta_submissions WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM delta_file_baselines WHERE project_id = ?")
            .bind(id)
            .execute(pool)
            .await;

        // Cascade delete gyp chat messages
        if let Ok(gyp_store) = crate::core::gyp_chat::GypChatStore::open().await {
            let _ = gyp_store.clear_project_messages(id).await;
        }

        // Delete project data directory (board, etc.)
        let project_dir = crate::core::config::hirsel_dir()
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

    /// Normalize StartingPoint to separate columns
    fn normalize_starting_point(
        sp: &StartingPoint,
    ) -> (String, Option<String>, Option<String>, Option<String>) {
        match sp {
            StartingPoint::Greenfield => ("greenfield".to_string(), None, None, None),
            StartingPoint::LocalFolder { path } => {
                ("local_folder".to_string(), Some(path.clone()), None, None)
            }
            StartingPoint::GitRepo { url, branch } => (
                "git_repo".to_string(),
                None,
                Some(url.clone()),
                branch.clone(),
            ),
        }
    }

    /// Denormalize database row to StartingPoint
    fn denormalize_starting_point(row: &SqliteRow) -> ProjectResult<StartingPoint> {
        let sp_type: String = row.get("starting_point_type");
        match sp_type.as_str() {
            "greenfield" => Ok(StartingPoint::Greenfield),
            "local_folder" => {
                let path: String = row.get("starting_point_path");
                Ok(StartingPoint::LocalFolder { path })
            }
            "git_repo" => {
                let url: String = row.get("starting_point_url");
                let branch: Option<String> = row.get("starting_point_branch");
                Ok(StartingPoint::GitRepo { url, branch })
            }
            _ => Err(ProjectError::Database(sqlx::Error::Decode(
                "Invalid starting_point_type".into(),
            ))),
        }
    }

    /// Convert database row to Project
    fn row_to_project(row: &SqliteRow) -> ProjectResult<Project> {
        Ok(Project {
            id: row.get("id"),
            name: row.get("name"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            starting_point: Self::denormalize_starting_point(row)?,
            worker_scale: row.get("worker_scale"),
            time_limit_minutes: row.get("time_limit_minutes"),
            human_in_the_loop: row.get::<i64, _>("human_in_the_loop") != 0,
            docs_path: row.get("docs_path"),
            persist_docs_changes: row.get::<i64, _>("persist_docs_changes") != 0,
            description: row.get("description"),
            target_branch: row.get("target_branch"),
            runner: row.get("runner"),
            x: row.get("x"),
            y: row.get("y"),
        })
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
