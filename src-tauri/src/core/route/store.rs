//! Route storage in global SQLite database

use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::files::RouteFiles;
use super::types::{CreateRouteRequest, Route, RouteTree};
use crate::core::db::{global_pool, utc_now};

/// Schema for routes table
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS routes (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    parent_route_id INTEGER REFERENCES routes(id),
    parent_version_id INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE(project_id, name)
);

CREATE INDEX IF NOT EXISTS idx_routes_project ON routes(project_id);
CREATE INDEX IF NOT EXISTS idx_routes_parent ON routes(parent_route_id);
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

    /// Create the main route for a new project
    ///
    /// Returns the route ID. Idempotent - returns existing main route if one exists.
    pub async fn create_main_route(&self) -> RouteResult<Route> {
        let pool = self.pool().await;

        // Check if main route already exists
        if let Some(route) = self.get_route_by_name("main").await? {
            return Ok(route);
        }

        let now = utc_now();
        let result = sqlx::query(
            "INSERT INTO routes (project_id, name, parent_route_id, parent_version_id, created_at)
             VALUES (?, 'main', NULL, NULL, ?)",
        )
        .bind(self.project_id)
        .bind(&now)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        self.get_route(id).await
    }

    /// Create a new route
    ///
    /// If parent_route_id is specified, copies all draft nodes from the parent route.
    pub async fn create_route(&self, req: &CreateRouteRequest) -> RouteResult<Route> {
        let pool = self.pool().await;

        // Check if route with this name already exists
        if self.get_route_by_name(&req.name).await?.is_some() {
            return Err(RouteError::AlreadyExists(req.name.clone()));
        }

        // Validate parent route exists if specified
        if let Some(parent_id) = req.parent_route_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM routes WHERE id = ? AND project_id = ?)",
            )
            .bind(parent_id)
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(RouteError::ParentNotFound(parent_id));
            }
        }

        let now = utc_now();
        let result = sqlx::query(
            "INSERT INTO routes (project_id, name, parent_route_id, parent_version_id, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(self.project_id)
        .bind(&req.name)
        .bind(req.parent_route_id)
        .bind(req.parent_version_id)
        .bind(&now)
        .execute(pool)
        .await?;

        let new_route_id = result.last_insert_rowid();

        // Initialize folder structure for the new route
        let new_route_files = RouteFiles::new(self.project_id, &req.name);
        if let Err(e) = new_route_files.init_dirs() {
            tracing::warn!("Failed to initialize route directories: {}", e);
        }

        // Fork: Copy draft nodes and files from parent route
        if let Some(parent_route_id) = req.parent_route_id {
            // Copy draft nodes in database
            self.copy_draft_nodes(parent_route_id, new_route_id).await?;

            // Copy files from parent route
            if let Ok(parent_route) = self.get_route(parent_route_id).await {
                let parent_files = RouteFiles::new(self.project_id, &parent_route.name);

                // Copy docs
                if let Err(e) = new_route_files.copy_docs_from(&parent_files) {
                    tracing::warn!("Failed to copy docs from parent route: {}", e);
                }

                // Copy board content files (tasks/*.md)
                if let Err(e) = new_route_files.copy_board_from(&parent_files) {
                    tracing::warn!("Failed to copy board files from parent route: {}", e);
                }
            }
        }

        self.get_route(new_route_id).await
    }

    /// Copy all draft nodes from one route to another
    ///
    /// Since primary key is (id, project_id, route_id), we can copy with the same IDs.
    /// Also copies the related tables: draft_node_validates, draft_node_blocked_by
    async fn copy_draft_nodes(&self, from_route_id: i64, to_route_id: i64) -> RouteResult<()> {
        let pool = self.pool().await;
        let now = utc_now();

        // Copy draft_nodes - same IDs, different route_id
        let result = sqlx::query(
            r#"
            INSERT INTO draft_nodes (
                id, project_id, route_id, parent_id, position, name, node_type, content,
                x, y, created_at, updated_at
            )
            SELECT
                id, project_id, ?, parent_id, position, name, node_type, content,
                x, y, created_at, ?
            FROM draft_nodes
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

        // Copy draft_node_validates
        sqlx::query(
            r#"
            INSERT INTO draft_node_validates (eval_id, task_id, project_id, route_id)
            SELECT eval_id, task_id, project_id, ?
            FROM draft_node_validates
            WHERE project_id = ? AND route_id = ?
            "#,
        )
        .bind(to_route_id)
        .bind(self.project_id)
        .bind(from_route_id)
        .execute(pool)
        .await?;

        // Copy draft_node_blocked_by
        sqlx::query(
            r#"
            INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id, route_id)
            SELECT node_id, blocker_id, project_id, ?
            FROM draft_node_blocked_by
            WHERE project_id = ? AND route_id = ?
            "#,
        )
        .bind(to_route_id)
        .bind(self.project_id)
        .bind(from_route_id)
        .execute(pool)
        .await?;

        tracing::info!(
            "Copied {} draft nodes from route {} to route {} for project {}",
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
            "SELECT id, project_id, name, parent_route_id, parent_version_id, created_at
             FROM routes WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| RouteError::NotFound(id.to_string()))?;

        Ok(Self::row_to_route(&row))
    }

    /// Get a route by name
    pub async fn get_route_by_name(&self, name: &str) -> RouteResult<Option<Route>> {
        let pool = self.pool().await;

        let row = sqlx::query(
            "SELECT id, project_id, name, parent_route_id, parent_version_id, created_at
             FROM routes WHERE name = ? AND project_id = ?",
        )
        .bind(name)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| Self::row_to_route(&r)))
    }

    /// List all routes for this project
    pub async fn list_routes(&self) -> RouteResult<Vec<Route>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            "SELECT id, project_id, name, parent_route_id, parent_version_id, created_at
             FROM routes WHERE project_id = ? ORDER BY created_at ASC",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_route).collect())
    }

    /// Get routes as a tree structure
    pub async fn get_route_tree(&self) -> RouteResult<Vec<RouteTree>> {
        let routes = self.list_routes().await?;
        Ok(Self::build_tree(routes))
    }

    /// Delete a route
    ///
    /// Cannot delete:
    /// - The main route
    /// - Routes that have child routes
    pub async fn delete_route(&self, id: i64) -> RouteResult<()> {
        let pool = self.pool().await;

        let route = self.get_route(id).await?;

        // Cannot delete main route
        if route.name == "main" {
            return Err(RouteError::CannotDeleteMain);
        }

        // Check for child routes
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

        // Delete route-scoped data first
        self.delete_route_data(pool, id).await?;

        // Delete the route
        sqlx::query("DELETE FROM routes WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Delete all route-scoped data for a route
    async fn delete_route_data(&self, pool: &SqlitePool, route_id: i64) -> RouteResult<()> {
        // Delete draft nodes and relationships
        sqlx::query("DELETE FROM draft_node_validates WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM draft_node_blocked_by WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM draft_nodes WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        // Delete live nodes and relationships
        sqlx::query("DELETE FROM live_node_validates WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM live_node_blocked_by WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM live_nodes WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        // Delete board versions and deliveries
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

        // Delete project runs
        sqlx::query("DELETE FROM project_runs WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        // Delete project messages
        sqlx::query("DELETE FROM project_messages WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        // Delete delta submissions
        sqlx::query("DELETE FROM delta_submissions WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await
            .ok();

        Ok(())
    }

    /// Convert database row to Route
    fn row_to_route(row: &sqlx::sqlite::SqliteRow) -> Route {
        Route {
            id: row.get("id"),
            project_id: row.get("project_id"),
            name: row.get("name"),
            parent_route_id: row.get("parent_route_id"),
            parent_version_id: row.get("parent_version_id"),
            created_at: row.get("created_at"),
        }
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
                        // Parent not found, treat as root
                        tree_nodes.insert(id, node);
                    }
                }
            }
        }

        // Remaining nodes are roots (main route and any orphans)
        roots.extend(tree_nodes.into_values());
        roots.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        roots
    }
}
