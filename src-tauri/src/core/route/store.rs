//! Route storage in global SQLite database

use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::files::RouteFiles;
use super::types::{
    CreateMainRouteRequest, CreateRouteRepoRequest, CreateRouteRequest, Route, RouteRepo,
    RouteTree, UpdateRouteRepoRequest, UpdateRouteSettingsRequest,
};
use crate::core::db::{global_pool, utc_now};
use crate::core::draft::StartingPoint;

/// Schema for route tables.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS routes (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    parent_route_id INTEGER REFERENCES routes(id),
    parent_version_id INTEGER,
    default_repo_id INTEGER,
    worker_scale TEXT,
    time_limit_minutes INTEGER,
    human_in_the_loop INTEGER NOT NULL DEFAULT 1,
    docs_path TEXT NOT NULL DEFAULT 'docs',
    persist_docs_changes INTEGER NOT NULL DEFAULT 1,
    target_branch TEXT,
    runner TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_id, name)
);

CREATE TABLE IF NOT EXISTS route_repos (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    name TEXT NOT NULL,

    source_type TEXT NOT NULL,
    source_path TEXT,
    source_url TEXT,
    source_branch TEXT,

    target_branch TEXT,
    runner TEXT,

    is_archived INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY(route_id) REFERENCES routes(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_routes_project ON routes(project_id);
CREATE INDEX IF NOT EXISTS idx_routes_parent ON routes(parent_route_id);
CREATE INDEX IF NOT EXISTS idx_route_repos_route_id ON route_repos(route_id);
CREATE INDEX IF NOT EXISTS idx_route_repos_route_archived ON route_repos(route_id, is_archived);
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

/// Error type for route operations
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Route not found: {0}")]
    NotFound(String),
    #[error("Route already exists: {0}")]
    AlreadyExists(String),
    #[error("Cannot delete main route")]
    CannotDeleteMain,
    #[error("Cannot delete route with children")]
    HasChildren,
    #[error("Parent route not found: {0}")]
    ParentNotFound(i64),
    #[error("Project not found: {0}")]
    ProjectNotFound(i64),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

pub type RouteResult<T> = Result<T, RouteError>;

/// Route store backed by global SQLite database
pub struct RouteStore {
    project_id: i64,
}

impl RouteStore {
    /// Create a new route store for a project
    pub async fn new(project_id: i64) -> RouteResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self { project_id })
    }

    /// Get the pool
    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    /// Create the main route with a supplied repo/settings seed.
    ///
    /// Returns the existing main route if it already exists.
    pub async fn create_main_route_with_seed(
        &self,
        req: &CreateMainRouteRequest,
    ) -> RouteResult<Route> {
        let pool = self.pool().await;

        if let Some(route) = self.get_route_by_name("main").await? {
            return Ok(route);
        }

        if req.repos.is_empty() {
            return Err(RouteError::InvalidInput(
                "Main route requires at least one repo".to_string(),
            ));
        }

        let now = utc_now();
        let result = sqlx::query(
            "INSERT INTO routes (
                project_id, name, parent_route_id, parent_version_id,
                default_repo_id,
                worker_scale, time_limit_minutes, human_in_the_loop,
                docs_path, persist_docs_changes, target_branch, runner,
                created_at, updated_at
            ) VALUES (?, 'main', NULL, NULL, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.project_id)
        .bind(&req.worker_scale)
        .bind(req.time_limit_minutes)
        .bind(req.human_in_the_loop.unwrap_or(true) as i64)
        .bind(req.docs_path.as_deref().unwrap_or("docs"))
        .bind(req.persist_docs_changes.unwrap_or(true) as i64)
        .bind(&req.target_branch)
        .bind(&req.runner)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        let route_id = result.last_insert_rowid();
        let default_repo_id = self
            .seed_route_repos(route_id, &req.repos, req.default_repo_index)
            .await?;

        sqlx::query("UPDATE routes SET default_repo_id = ?, updated_at = ? WHERE id = ?")
            .bind(default_repo_id)
            .bind(utc_now())
            .bind(route_id)
            .execute(pool)
            .await?;

        let route = self.get_route(route_id).await?;

        self.create_root_node(route_id).await?;

        let route_files = RouteFiles::new(self.project_id, &route.name);
        if let Err(e) = route_files.init_dirs() {
            tracing::warn!("Failed to initialize route directories: {}", e);
        }

        Ok(route)
    }

    /// Create a default main route (fallback for legacy callers).
    pub async fn create_main_route(&self) -> RouteResult<Route> {
        self.create_main_route_with_seed(&CreateMainRouteRequest {
            repos: vec![CreateRouteRepoRequest {
                name: Some("workspace".to_string()),
                starting_point: StartingPoint::Greenfield,
                target_branch: Some("main".to_string()),
                runner: None,
            }],
            default_repo_index: Some(0),
            worker_scale: None,
            time_limit_minutes: None,
            human_in_the_loop: Some(true),
            docs_path: Some("docs".to_string()),
            persist_docs_changes: Some(true),
            target_branch: Some("main".to_string()),
            runner: None,
        })
        .await
    }

    async fn seed_route_repos(
        &self,
        route_id: i64,
        repos: &[CreateRouteRepoRequest],
        default_repo_index: Option<usize>,
    ) -> RouteResult<i64> {
        let pool = self.pool().await;

        let mut created_ids = Vec::with_capacity(repos.len());
        for (idx, repo_req) in repos.iter().enumerate() {
            let repo_name = repo_req
                .name
                .clone()
                .unwrap_or_else(|| default_repo_name(idx, &repo_req.starting_point));
            let repo = self
                .create_route_repo_inner(pool, route_id, repo_name, repo_req)
                .await?;
            created_ids.push(repo.id);
        }

        let default_idx = default_repo_index
            .unwrap_or(0)
            .min(created_ids.len().saturating_sub(1));
        Ok(created_ids[default_idx])
    }

    /// Create the root feature node for a route's board tree.
    ///
    /// The root node is named after the project and cannot be deleted.
    /// Idempotent via `create_node_with_id`.
    async fn create_root_node(&self, route_id: i64) -> RouteResult<()> {
        use crate::core::delta::types::{
            BoardNodeDifficulty, BoardNodeSource, BoardNodeStatus, NodeKind,
        };
        use crate::core::delta::DeltaState;

        let pool = self.pool().await;

        // Get project name for the root node
        let project_name: String = sqlx::query_scalar("SELECT name FROM projects WHERE id = ?")
            .bind(self.project_id)
            .fetch_one(pool)
            .await
            .map_err(|_| RouteError::ProjectNotFound(self.project_id))?;

        // Kebab-case slug for root node ID
        let root_id = project_name
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-");

        let delta_state = DeltaState::with_route(self.project_id, route_id);
        delta_state
            .create_node_with_id(
                &root_id,
                None, // no parent — this IS the root
                &project_name,
                NodeKind::Feature,
                BoardNodeSource::System,
                "",
                BoardNodeDifficulty::Medium,
                BoardNodeStatus::Draft,
                &[],
                &[],
            )
            .await
            .map_err(|e| RouteError::Database(sqlx::Error::Protocol(e.to_string())))?;

        Ok(())
    }

    /// Create a new route by forking a parent route.
    ///
    /// If parent_route_id is specified, copies all board nodes, repos, docs/code files,
    /// and route settings from the parent route.
    pub async fn create_route(&self, req: &CreateRouteRequest) -> RouteResult<Route> {
        let pool = self.pool().await;
        let name = req.name.trim();
        if name.is_empty() {
            return Err(RouteError::InvalidInput(
                "Route name cannot be empty".to_string(),
            ));
        }

        if self.get_route_by_name(name).await?.is_some() {
            return Err(RouteError::AlreadyExists(name.to_string()));
        }

        let parent_id = if let Some(id) = req.parent_route_id {
            id
        } else if let Some(main) = self.get_route_by_name("main").await? {
            main.id
        } else {
            return Err(RouteError::InvalidInput(
                "Cannot create a route without a parent route".to_string(),
            ));
        };

        let parent_route = self.get_route(parent_id).await?;

        let now = utc_now();
        let result = sqlx::query(
            "INSERT INTO routes (
                project_id, name, parent_route_id, parent_version_id,
                default_repo_id,
                worker_scale, time_limit_minutes, human_in_the_loop,
                docs_path, persist_docs_changes, target_branch, runner,
                created_at, updated_at
             ) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.project_id)
        .bind(name)
        .bind(Some(parent_id))
        .bind(req.parent_version_id)
        .bind(&parent_route.worker_scale)
        .bind(parent_route.time_limit_minutes)
        .bind(parent_route.human_in_the_loop as i64)
        .bind(&parent_route.docs_path)
        .bind(parent_route.persist_docs_changes as i64)
        .bind(&parent_route.target_branch)
        .bind(&parent_route.runner)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        let new_route_id = result.last_insert_rowid();

        let new_default_repo_id = self.copy_route_repos(parent_id, new_route_id).await?;
        sqlx::query("UPDATE routes SET default_repo_id = ?, updated_at = ? WHERE id = ?")
            .bind(new_default_repo_id)
            .bind(utc_now())
            .bind(new_route_id)
            .execute(pool)
            .await?;

        // Initialize folder structure for the new route
        let new_route_files = RouteFiles::new(self.project_id, name);
        if let Err(e) = new_route_files.init_dirs() {
            tracing::warn!("Failed to initialize route directories: {}", e);
        }

        // Copy board nodes in database
        self.copy_board_nodes(parent_id, new_route_id).await?;

        // Copy files from parent route
        let parent_files = RouteFiles::new(self.project_id, &parent_route.name);

        if let Err(e) = new_route_files.copy_docs_from(&parent_files) {
            tracing::warn!("Failed to copy docs from parent route: {}", e);
        }

        if let Err(e) = new_route_files.copy_code_from(&parent_files) {
            tracing::warn!("Failed to copy code from parent route: {}", e);
        }

        self.get_route(new_route_id).await
    }

    async fn copy_route_repos(&self, from_route_id: i64, to_route_id: i64) -> RouteResult<i64> {
        let pool = self.pool().await;

        let parent = self.get_route(from_route_id).await?;
        let repos = self.list_route_repos(from_route_id).await?;
        if repos.is_empty() {
            return Err(RouteError::InvalidInput(format!(
                "Parent route {} has no active repos",
                from_route_id
            )));
        }

        let mut copied_default: Option<i64> = None;
        for repo in repos {
            let created = self
                .create_route_repo_inner(
                    pool,
                    to_route_id,
                    repo.name,
                    &CreateRouteRepoRequest {
                        name: None,
                        starting_point: repo.starting_point,
                        target_branch: repo.target_branch,
                        runner: repo.runner,
                    },
                )
                .await?;

            if Some(repo.id) == parent.default_repo_id {
                copied_default = Some(created.id);
            }
        }

        copied_default.ok_or_else(|| RouteError::InvalidInput("Failed to copy default repo".into()))
    }

    /// Copy all board nodes from one route to another
    ///
    /// Since primary key is (id, project_id, route_id), we can copy with the same IDs.
    /// Also copies the related tables: board_node_checked_by, board_node_blocked_by
    async fn copy_board_nodes(&self, from_route_id: i64, to_route_id: i64) -> RouteResult<()> {
        let pool = self.pool().await;
        let now = utc_now();

        // Copy board_nodes - same IDs, different route_id
        let result = sqlx::query(
            r#"
            INSERT INTO board_nodes (
                id, project_id, route_id, parent_id, position, name, kind, source, content, status,
                difficulty, x, y, created_at, updated_at
            )
            SELECT
                id, project_id, ?, parent_id, position, name, kind, source, content, status,
                difficulty, x, y, created_at, ?
            FROM board_nodes
            WHERE project_id = ? AND route_id = ?
            "#,
        )
        .bind(to_route_id)
        .bind(&now)
        .bind(self.project_id)
        .bind(from_route_id)
        .execute(pool)
        .await?;

        let nodes_copied = result.rows_affected();

        // Copy board_node_checked_by
        sqlx::query(
            r#"
            INSERT INTO board_node_checked_by (check_id, node_id, project_id, route_id)
            SELECT check_id, node_id, project_id, ?
            FROM board_node_checked_by
            WHERE project_id = ? AND route_id = ?
            "#,
        )
        .bind(to_route_id)
        .bind(self.project_id)
        .bind(from_route_id)
        .execute(pool)
        .await?;

        // Copy board_node_blocked_by
        sqlx::query(
            r#"
            INSERT INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id)
            SELECT node_id, blocker_id, project_id, ?
            FROM board_node_blocked_by
            WHERE project_id = ? AND route_id = ?
            "#,
        )
        .bind(to_route_id)
        .bind(self.project_id)
        .bind(from_route_id)
        .execute(pool)
        .await?;

        tracing::info!(
            "Copied {} board nodes from route {} to route {} for project {}",
            nodes_copied,
            from_route_id,
            to_route_id,
            self.project_id
        );

        Ok(())
    }

    /// Get a route by ID
    pub async fn get_route(&self, id: i64) -> RouteResult<Route> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, project_id, name, parent_route_id, parent_version_id,
                    default_repo_id, worker_scale, time_limit_minutes, human_in_the_loop,
                    docs_path, persist_docs_changes, target_branch, runner,
                    created_at, updated_at
             FROM routes WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| RouteError::NotFound(id.to_string()))?;

        self.row_to_route(&row).await
    }

    /// Get a route by name
    pub async fn get_route_by_name(&self, name: &str) -> RouteResult<Option<Route>> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, project_id, name, parent_route_id, parent_version_id,
                    default_repo_id, worker_scale, time_limit_minutes, human_in_the_loop,
                    docs_path, persist_docs_changes, target_branch, runner,
                    created_at, updated_at
             FROM routes WHERE lower(name) = lower(?) AND project_id = ?",
        )
        .bind(name)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(self.row_to_route(&r).await?)),
            None => Ok(None),
        }
    }

    /// List all routes for this project
    pub async fn list_routes(&self) -> RouteResult<Vec<Route>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            "SELECT id, project_id, name, parent_route_id, parent_version_id,
                    default_repo_id, worker_scale, time_limit_minutes, human_in_the_loop,
                    docs_path, persist_docs_changes, target_branch, runner,
                    created_at, updated_at
             FROM routes WHERE project_id = ? ORDER BY created_at ASC",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let mut routes = Vec::with_capacity(rows.len());
        for row in rows {
            routes.push(self.row_to_route(&row).await?);
        }
        Ok(routes)
    }

    /// Get routes as a tree structure
    pub async fn get_route_tree(&self) -> RouteResult<Vec<RouteTree>> {
        let routes = self.list_routes().await?;
        Ok(Self::build_tree(routes))
    }

    /// Update route settings.
    pub async fn update_route_settings(
        &self,
        route_id: i64,
        req: &UpdateRouteSettingsRequest,
    ) -> RouteResult<Route> {
        let pool = self.pool().await;

        let _ = self.get_route(route_id).await?;

        let now = utc_now();
        let mut updates = vec!["updated_at = ?".to_string()];
        let mut bind_index = 2usize;

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
        if req.target_branch.is_some() {
            updates.push(format!("target_branch = ?{}", bind_index));
            bind_index += 1;
        }
        if req.runner.is_some() {
            updates.push(format!("runner = ?{}", bind_index));
            bind_index += 1;
        }

        let sql = format!(
            "UPDATE routes SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            bind_index,
            bind_index + 1
        );

        let mut query = sqlx::query(&sql).bind(&now);

        if let Some(ref worker_scale) = req.worker_scale {
            query = query.bind(worker_scale);
        }
        if let Some(time_limit_minutes) = req.time_limit_minutes {
            query = query.bind(time_limit_minutes);
        }
        if let Some(human_in_the_loop) = req.human_in_the_loop {
            query = query.bind(human_in_the_loop as i64);
        }
        if let Some(ref docs_path) = req.docs_path {
            query = query.bind(docs_path);
        }
        if let Some(persist_docs_changes) = req.persist_docs_changes {
            query = query.bind(persist_docs_changes as i64);
        }
        if let Some(ref target_branch) = req.target_branch {
            query = query.bind(target_branch);
        }
        if let Some(ref runner) = req.runner {
            query = query.bind(runner);
        }

        query = query.bind(route_id).bind(self.project_id);
        query.execute(pool).await?;

        self.get_route(route_id).await
    }

    pub async fn list_route_repos(&self, route_id: i64) -> RouteResult<Vec<RouteRepo>> {
        let pool = self.pool().await;
        self.list_route_repos_internal(pool, route_id).await
    }

    pub async fn get_route_repo(&self, route_id: i64, repo_id: i64) -> RouteResult<RouteRepo> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, project_id, route_id, name,
                    source_type, source_path, source_url, source_branch,
                    target_branch, runner, is_archived,
                    created_at, updated_at
             FROM route_repos
             WHERE id = ? AND route_id = ? AND project_id = ?",
        )
        .bind(repo_id)
        .bind(route_id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| RouteError::NotFound(format!("Repo {}", repo_id)))?;

        row_to_repo(&row)
    }

    pub async fn create_route_repo(
        &self,
        route_id: i64,
        req: &CreateRouteRepoRequest,
    ) -> RouteResult<RouteRepo> {
        let pool = self.pool().await;

        let _ = self.get_route(route_id).await?;

        let existing_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM route_repos WHERE route_id = ? AND is_archived = 0",
        )
        .bind(route_id)
        .fetch_one(pool)
        .await?;

        let repo_name = req
            .name
            .clone()
            .unwrap_or_else(|| default_repo_name(existing_count as usize, &req.starting_point));

        let repo = self
            .create_route_repo_inner(pool, route_id, repo_name, req)
            .await?;

        let route = self.get_route(route_id).await?;
        if route.default_repo_id.is_none() {
            sqlx::query("UPDATE routes SET default_repo_id = ?, updated_at = ? WHERE id = ?")
                .bind(repo.id)
                .bind(utc_now())
                .bind(route_id)
                .execute(pool)
                .await?;
        }

        Ok(repo)
    }

    pub async fn update_route_repo(
        &self,
        route_id: i64,
        repo_id: i64,
        req: &UpdateRouteRepoRequest,
    ) -> RouteResult<RouteRepo> {
        let pool = self.pool().await;

        let _ = self.get_route(route_id).await?;

        let now = utc_now();
        let mut updates = vec!["updated_at = ?".to_string()];
        let mut bind_index = 2usize;

        if req.name.is_some() {
            updates.push(format!("name = ?{}", bind_index));
            bind_index += 1;
        }
        if req.starting_point.is_some() {
            updates.push(format!("source_type = ?{}", bind_index));
            bind_index += 1;
            updates.push(format!("source_path = ?{}", bind_index));
            bind_index += 1;
            updates.push(format!("source_url = ?{}", bind_index));
            bind_index += 1;
            updates.push(format!("source_branch = ?{}", bind_index));
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
        if req.is_archived.is_some() {
            updates.push(format!("is_archived = ?{}", bind_index));
            bind_index += 1;
        }

        let sql = format!(
            "UPDATE route_repos SET {} WHERE id = ?{} AND route_id = ?{} AND project_id = ?{}",
            updates.join(", "),
            bind_index,
            bind_index + 1,
            bind_index + 2
        );

        let mut query = sqlx::query(&sql).bind(&now);

        if let Some(ref name) = req.name {
            query = query.bind(name);
        }

        if let Some(ref sp) = req.starting_point {
            let (source_type, source_path, source_url, source_branch) =
                normalize_starting_point(sp);
            query = query
                .bind(source_type)
                .bind(source_path)
                .bind(source_url)
                .bind(source_branch);
        }

        if let Some(ref target_branch) = req.target_branch {
            query = query.bind(target_branch);
        }

        if let Some(ref runner) = req.runner {
            query = query.bind(runner);
        }

        if let Some(is_archived) = req.is_archived {
            query = query.bind(is_archived as i64);
        }

        query = query.bind(repo_id).bind(route_id).bind(self.project_id);
        query.execute(pool).await?;

        self.get_route_repo(route_id, repo_id).await
    }

    pub async fn delete_route_repo(&self, route_id: i64, repo_id: i64) -> RouteResult<()> {
        let pool = self.pool().await;

        let _ = self.get_route(route_id).await?;

        let active_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM route_repos WHERE route_id = ? AND is_archived = 0",
        )
        .bind(route_id)
        .fetch_one(pool)
        .await?;

        if active_count <= 1 {
            return Err(RouteError::InvalidInput(
                "Cannot archive the last active repo in a route".to_string(),
            ));
        }

        let now = utc_now();
        sqlx::query(
            "UPDATE route_repos SET is_archived = 1, updated_at = ? WHERE id = ? AND route_id = ? AND project_id = ?",
        )
        .bind(now)
        .bind(repo_id)
        .bind(route_id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        let route = self.get_route(route_id).await?;
        if route.default_repo_id == Some(repo_id) {
            let fallback_repo_id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM route_repos WHERE route_id = ? AND is_archived = 0 ORDER BY id LIMIT 1",
            )
            .bind(route_id)
            .fetch_optional(pool)
            .await?;

            sqlx::query("UPDATE routes SET default_repo_id = ?, updated_at = ? WHERE id = ?")
                .bind(fallback_repo_id)
                .bind(utc_now())
                .bind(route_id)
                .execute(pool)
                .await?;
        }

        Ok(())
    }

    pub async fn set_default_route_repo(&self, route_id: i64, repo_id: i64) -> RouteResult<Route> {
        let pool = self.pool().await;

        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM route_repos
                WHERE id = ? AND route_id = ? AND project_id = ? AND is_archived = 0
            )",
        )
        .bind(repo_id)
        .bind(route_id)
        .bind(self.project_id)
        .fetch_one(pool)
        .await?;

        if !exists {
            return Err(RouteError::NotFound(format!(
                "Repo {} not found for route {}",
                repo_id, route_id
            )));
        }

        sqlx::query("UPDATE routes SET default_repo_id = ?, updated_at = ? WHERE id = ?")
            .bind(repo_id)
            .bind(utc_now())
            .bind(route_id)
            .execute(pool)
            .await?;

        self.get_route(route_id).await
    }

    pub async fn get_default_repo_starting_point(
        &self,
        route_id: i64,
    ) -> RouteResult<StartingPoint> {
        let route = self.get_route(route_id).await?;

        let default_repo = if let Some(id) = route.default_repo_id {
            route.repos.iter().find(|r| r.id == id)
        } else {
            route.repos.first()
        }
        .ok_or_else(|| {
            RouteError::InvalidInput(format!("Route {} has no active repos", route_id))
        })?;

        Ok(default_repo.starting_point.clone())
    }

    async fn create_route_repo_inner(
        &self,
        pool: &SqlitePool,
        route_id: i64,
        name: String,
        req: &CreateRouteRepoRequest,
    ) -> RouteResult<RouteRepo> {
        let now = utc_now();
        let (source_type, source_path, source_url, source_branch) =
            normalize_starting_point(&req.starting_point);

        let insert = sqlx::query(
            "INSERT INTO route_repos (
                project_id, route_id, name,
                source_type, source_path, source_url, source_branch,
                target_branch, runner, is_archived, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(self.project_id)
        .bind(route_id)
        .bind(name)
        .bind(source_type)
        .bind(source_path)
        .bind(source_url)
        .bind(source_branch)
        .bind(&req.target_branch)
        .bind(&req.runner)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        let repo_id = insert.last_insert_rowid();
        self.get_route_repo(route_id, repo_id).await
    }

    async fn list_route_repos_internal(
        &self,
        pool: &SqlitePool,
        route_id: i64,
    ) -> RouteResult<Vec<RouteRepo>> {
        let rows = sqlx::query(
            "SELECT id, project_id, route_id, name,
                    source_type, source_path, source_url, source_branch,
                    target_branch, runner, is_archived,
                    created_at, updated_at
             FROM route_repos
             WHERE project_id = ? AND route_id = ? AND is_archived = 0
             ORDER BY id ASC",
        )
        .bind(self.project_id)
        .bind(route_id)
        .fetch_all(pool)
        .await?;

        let mut repos = Vec::with_capacity(rows.len());
        for row in rows {
            repos.push(row_to_repo(&row)?);
        }
        Ok(repos)
    }

    async fn row_to_route(&self, row: &sqlx::sqlite::SqliteRow) -> RouteResult<Route> {
        let route_id: i64 = row.get("id");
        let pool = self.pool().await;
        let repos = self.list_route_repos_internal(pool, route_id).await?;

        Ok(Route {
            id: route_id,
            project_id: row.get("project_id"),
            name: row.get("name"),
            parent_route_id: row.get("parent_route_id"),
            parent_version_id: row.get("parent_version_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            repos,
            default_repo_id: row.get("default_repo_id"),
            worker_scale: row.get("worker_scale"),
            time_limit_minutes: row.get("time_limit_minutes"),
            human_in_the_loop: row.get::<i64, _>("human_in_the_loop") != 0,
            docs_path: row.get("docs_path"),
            persist_docs_changes: row.get::<i64, _>("persist_docs_changes") != 0,
            target_branch: row.get("target_branch"),
            runner: row.get("runner"),
        })
    }

    /// Delete a route
    ///
    /// Cannot delete:
    /// - The main route
    /// - Routes that have child routes
    pub async fn delete_route(&self, id: i64) -> RouteResult<()> {
        let pool = self.pool().await;

        let route = self.get_route(id).await?;

        if route.name == "main" {
            return Err(RouteError::CannotDeleteMain);
        }

        let has_children: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM routes WHERE parent_route_id = ? AND project_id = ?)",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_one(pool)
        .await?;

        if has_children {
            return Err(RouteError::HasChildren);
        }

        self.delete_route_data(pool, id).await?;

        sqlx::query("DELETE FROM routes WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Delete all route-scoped data for a route
    async fn delete_route_data(&self, pool: &SqlitePool, route_id: i64) -> RouteResult<()> {
        sqlx::query("DELETE FROM board_node_checked_by WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM board_node_blocked_by WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM board_nodes WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        sqlx::query("DELETE FROM delivery_attempts WHERE delivery_id IN (SELECT id FROM deliveries WHERE route_id = ?)")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM deliveries WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM board_versions WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        sqlx::query("DELETE FROM project_runs WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        sqlx::query("DELETE FROM project_messages WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        sqlx::query("DELETE FROM route_repos WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        Ok(())
    }

    /// Build tree structure from flat list
    fn build_tree(routes: Vec<Route>) -> Vec<RouteTree> {
        use std::collections::HashMap;

        let mut tree_nodes: HashMap<i64, RouteTree> =
            routes.into_iter().map(|r| (r.id, r.into())).collect();

        let mut roots = Vec::new();
        let ids: Vec<i64> = tree_nodes.keys().copied().collect();

        for id in ids {
            let parent_id = tree_nodes.get(&id).and_then(|n| n.parent_route_id);
            if let Some(pid) = parent_id {
                if let Some(node) = tree_nodes.remove(&id) {
                    if let Some(parent) = tree_nodes.get_mut(&pid) {
                        parent.children.push(node);
                    } else {
                        tree_nodes.insert(id, node);
                    }
                }
            }
        }

        roots.extend(tree_nodes.into_values());
        roots.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        roots
    }
}

fn infer_repo_name(url: &str) -> Option<String> {
    let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
    trimmed.rsplit('/').next().map(|s| s.to_string())
}

fn default_repo_name(index: usize, starting_point: &StartingPoint) -> String {
    match starting_point {
        StartingPoint::Greenfield => format!("workspace-{}", index + 1),
        StartingPoint::LocalFolder { .. } => format!("local-{}", index + 1),
        StartingPoint::GitRepo { url, .. } => {
            infer_repo_name(url).unwrap_or_else(|| format!("repo-{}", index + 1))
        }
    }
}

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

fn denormalize_starting_point(row: &sqlx::sqlite::SqliteRow) -> RouteResult<StartingPoint> {
    let sp_type: String = row.get("source_type");
    match sp_type.as_str() {
        "greenfield" => Ok(StartingPoint::Greenfield),
        "local_folder" => {
            let path: String = row.get("source_path");
            Ok(StartingPoint::LocalFolder { path })
        }
        "git_repo" => {
            let url: String = row.get("source_url");
            let branch: Option<String> = row.get("source_branch");
            Ok(StartingPoint::GitRepo { url, branch })
        }
        _ => Err(RouteError::InvalidInput(format!(
            "Invalid source_type '{}' for route repo",
            sp_type
        ))),
    }
}

fn row_to_repo(row: &sqlx::sqlite::SqliteRow) -> RouteResult<RouteRepo> {
    Ok(RouteRepo {
        id: row.get("id"),
        project_id: row.get("project_id"),
        route_id: row.get("route_id"),
        name: row.get("name"),
        starting_point: denormalize_starting_point(row)?,
        target_branch: row.get("target_branch"),
        runner: row.get("runner"),
        is_archived: row.get::<i64, _>("is_archived") != 0,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
