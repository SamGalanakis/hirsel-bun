//! Database operations for draft/live trees and delta submissions
//!
//! This module handles all SQLite operations for the delta dispatch system.

use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use tokio::sync::OnceCell;

use super::types::*;
use crate::core::db::{global_pool, utc_now};
use crate::core::names::slugify;

/// Schema for delta tables
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS draft_nodes (
    id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    parent_id TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    node_type TEXT NOT NULL DEFAULT 'task',
    content TEXT NOT NULL DEFAULT '',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (id, project_id)
);

CREATE INDEX IF NOT EXISTS idx_draft_nodes_project ON draft_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_draft_nodes_parent ON draft_nodes(parent_id);

CREATE TABLE IF NOT EXISTS draft_node_validates (
    eval_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    PRIMARY KEY (project_id, eval_id, task_id)
);
CREATE INDEX IF NOT EXISTS idx_draft_validates_eval ON draft_node_validates(eval_id);
CREATE INDEX IF NOT EXISTS idx_draft_validates_task ON draft_node_validates(task_id);

CREATE TABLE IF NOT EXISTS draft_node_blocked_by (
    node_id TEXT NOT NULL,
    blocker_id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    PRIMARY KEY (project_id, node_id, blocker_id)
);
CREATE INDEX IF NOT EXISTS idx_draft_blocked_node ON draft_node_blocked_by(node_id);
CREATE INDEX IF NOT EXISTS idx_draft_blocked_blocker ON draft_node_blocked_by(blocker_id);

CREATE TABLE IF NOT EXISTS live_nodes (
    id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    draft_node_id TEXT,
    parent_id TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    node_type TEXT NOT NULL DEFAULT 'task',
    content TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    source TEXT NOT NULL DEFAULT 'spec',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    last_commit_sha TEXT,
    -- Orchestration fields
    claimed_by TEXT,
    claimed_at TEXT,
    completed_by TEXT,
    eval_result TEXT,
    eval_feedback TEXT,
    tokens_used INTEGER,
    PRIMARY KEY (id, project_id)
);

CREATE INDEX IF NOT EXISTS idx_live_nodes_project ON live_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_live_nodes_parent ON live_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_live_nodes_project_status ON live_nodes(project_id, status);
CREATE INDEX IF NOT EXISTS idx_live_nodes_claimed ON live_nodes(project_id, claimed_by) WHERE status = 'working';

CREATE TABLE IF NOT EXISTS live_node_validates (
    eval_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    PRIMARY KEY (project_id, eval_id, task_id)
);
CREATE INDEX IF NOT EXISTS idx_live_validates_eval ON live_node_validates(eval_id);
CREATE INDEX IF NOT EXISTS idx_live_validates_task ON live_node_validates(task_id);

CREATE TABLE IF NOT EXISTS live_node_blocked_by (
    node_id TEXT NOT NULL,
    blocker_id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    PRIMARY KEY (project_id, node_id, blocker_id)
);
CREATE INDEX IF NOT EXISTS idx_live_blocked_node ON live_node_blocked_by(node_id);
CREATE INDEX IF NOT EXISTS idx_live_blocked_blocker ON live_node_blocked_by(blocker_id);

CREATE TABLE IF NOT EXISTS delta_submissions (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    batch_id INTEGER,
    delta_type TEXT NOT NULL,
    draft_node_id TEXT,
    live_node_id TEXT,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    refs TEXT DEFAULT '[]',
    created_at TEXT NOT NULL,
    processed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_delta_submissions_project ON delta_submissions(project_id);
CREATE INDEX IF NOT EXISTS idx_delta_submissions_batch ON delta_submissions(batch_id);

CREATE TABLE IF NOT EXISTS project_runs (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL UNIQUE,
    run_name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'paused',
    created_at TEXT NOT NULL,
    last_dispatch_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_project_runs_project ON project_runs(project_id);

CREATE TABLE IF NOT EXISTS board_versions (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    batch_id INTEGER NOT NULL,
    version_number INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    description TEXT,
    UNIQUE(project_id, version_number)
);

CREATE INDEX IF NOT EXISTS idx_board_versions_project ON board_versions(project_id);

CREATE TABLE IF NOT EXISTS deliveries (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    version_id INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    target_branch TEXT NOT NULL,
    delivery_branch TEXT,
    pr_url TEXT,
    pr_number INTEGER,
    started_at TEXT,
    completed_at TEXT,
    failure_reason TEXT,
    FOREIGN KEY (version_id) REFERENCES board_versions(id)
);

CREATE INDEX IF NOT EXISTS idx_deliveries_project ON deliveries(project_id);
CREATE INDEX IF NOT EXISTS idx_deliveries_version ON deliveries(version_id);
CREATE INDEX IF NOT EXISTS idx_deliveries_status ON deliveries(status);

CREATE TABLE IF NOT EXISTS delivery_attempts (
    id INTEGER PRIMARY KEY,
    delivery_id INTEGER NOT NULL,
    attempt_number INTEGER NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    error_message TEXT,
    FOREIGN KEY (delivery_id) REFERENCES deliveries(id)
);

CREATE INDEX IF NOT EXISTS idx_delivery_attempts_delivery ON delivery_attempts(delivery_id);
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

/// Error type for delta state operations
#[derive(Debug, thiserror::Error)]
pub enum DeltaStateError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Draft node not found: {0}")]
    DraftNodeNotFound(String),
    #[error("Live node not found: {0}")]
    LiveNodeNotFound(String),
    #[error("Project run not found for project: {0}")]
    ProjectRunNotFound(i64),
    #[error("Parent node not found: {0}")]
    ParentNodeNotFound(String),
    #[error("Eval nodes must validate at least one task")]
    EvalValidatesEmpty,
}

pub type DeltaStateResult<T> = Result<T, DeltaStateError>;

/// State manager for delta operations
pub struct DeltaState {
    project_id: i64,
}

impl DeltaState {
    /// Create a new delta state manager for a project
    pub fn new(project_id: i64) -> Self {
        Self { project_id }
    }

    /// Get the global pool with schema initialized
    async fn pool(&self) -> DeltaStateResult<&'static SqlitePool> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(pool)
    }

    /// Generate a unique slug ID (unique within this project)
    async fn generate_slug(
        &self,
        pool: &SqlitePool,
        table: &str,
        name: &str,
    ) -> DeltaStateResult<String> {
        let base_slug = slugify(name);
        let slug = if base_slug.is_empty() {
            "node".to_string()
        } else {
            base_slug
        };

        let mut candidate = slug.clone();
        let mut counter = 1;
        loop {
            let exists: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ? AND project_id = ?)",
                table
            ))
            .bind(&candidate)
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;

            if !exists {
                return Ok(candidate);
            }

            counter += 1;
            candidate = format!("{}-{}", slug, counter);
        }
    }

    // =========================================================================
    // Junction Table Helpers
    // =========================================================================

    /// Load all draft validates relationships (eval -> tasks)
    async fn load_draft_validates(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows =
            sqlx::query("SELECT eval_id, task_id FROM draft_node_validates WHERE project_id = ?")
                .bind(self.project_id)
                .fetch_all(pool)
                .await?;

        for row in rows {
            let eval_id: String = row.get("eval_id");
            let task_id: String = row.get("task_id");
            map.entry(eval_id).or_default().push(task_id);
        }
        Ok(map)
    }

    /// Load all draft blocked_by relationships (node -> blockers)
    async fn load_draft_blocked_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT node_id, blocker_id FROM draft_node_blocked_by WHERE project_id = ?",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let node_id: String = row.get("node_id");
            let blocker_id: String = row.get("blocker_id");
            map.entry(node_id).or_default().push(blocker_id);
        }
        Ok(map)
    }

    /// Load validates for a single draft node
    async fn load_draft_node_validates(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT task_id FROM draft_node_validates WHERE eval_id = ? AND project_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load blocked_by for a single draft node
    async fn load_draft_node_blocked_by(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT blocker_id FROM draft_node_blocked_by WHERE node_id = ? AND project_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load all live validates relationships (eval -> tasks)
    async fn load_live_validates(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows =
            sqlx::query("SELECT eval_id, task_id FROM live_node_validates WHERE project_id = ?")
                .bind(self.project_id)
                .fetch_all(pool)
                .await?;

        for row in rows {
            let eval_id: String = row.get("eval_id");
            let task_id: String = row.get("task_id");
            map.entry(eval_id).or_default().push(task_id);
        }
        Ok(map)
    }

    /// Load all live blocked_by relationships (node -> blockers)
    async fn load_live_blocked_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT node_id, blocker_id FROM live_node_blocked_by WHERE project_id = ?",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let node_id: String = row.get("node_id");
            let blocker_id: String = row.get("blocker_id");
            map.entry(node_id).or_default().push(blocker_id);
        }
        Ok(map)
    }

    /// Load validates for a single live node
    async fn load_live_node_validates(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT task_id FROM live_node_validates WHERE eval_id = ? AND project_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load blocked_by for a single live node
    async fn load_live_node_blocked_by(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT blocker_id FROM live_node_blocked_by WHERE node_id = ? AND project_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    // =========================================================================
    // Draft Node Operations
    // =========================================================================

    /// Get all draft nodes as flat list
    pub async fn get_draft_nodes(&self) -> DeltaStateResult<Vec<DraftNode>> {
        let pool = self.pool().await?;

        // Load relationships first
        let validates_map = self.load_draft_validates(pool).await?;
        let blocked_by_map = self.load_draft_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE project_id = ?
             ORDER BY parent_id NULLS FIRST, position",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                DraftNode {
                    id: id.clone(),
                    project_id: row.get("project_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Get a single draft node
    pub async fn get_draft_node(&self, id: &str) -> DeltaStateResult<DraftNode> {
        let pool = self.pool().await?;

        let validates = self.load_draft_node_validates(pool, id).await?;
        let blocked_by = self.load_draft_node_blocked_by(pool, id).await?;

        let row = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::DraftNodeNotFound(id.to_string()))?;

        Ok(DraftNode {
            id: row.get("id"),
            project_id: row.get("project_id"),
            parent_id: row.get("parent_id"),
            position: row.get("position"),
            name: row.get("name"),
            node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
            content: row.get("content"),
            validates,
            blocked_by,
            x: row.get("x"),
            y: row.get("y"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// Create a new draft node
    ///
    /// Validation rules:
    /// - If parent_id is provided, it must reference an existing node
    /// - If parent_id is not provided, auto-assign to root node (if one exists)
    /// - Eval nodes must have at least one task in validates
    pub async fn create_draft_node(
        &self,
        req: &CreateDraftNodeRequest,
    ) -> DeltaStateResult<DraftNode> {
        let pool = self.pool().await?;

        // Validate: Eval nodes must have non-empty validates
        if req.node_type == NodeType::Eval && req.validates.is_empty() {
            return Err(DeltaStateError::EvalValidatesEmpty);
        }

        // Validate parent_id if provided
        if let Some(pid) = &req.parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ? AND project_id = ?)",
            )
            .bind(pid)
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.clone()));
            }
        }
        let parent_id = req.parent_id.clone();

        let id = self.generate_slug(pool, "draft_nodes", &req.name).await?;
        let now = utc_now();

        // Get position (append to end of siblings)
        let position: i32 = match &parent_id {
            Some(pid) => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM draft_nodes WHERE parent_id = ? AND project_id = ?",
                )
                .bind(pid)
                .bind(self.project_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
            None => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM draft_nodes WHERE parent_id IS NULL AND project_id = ?",
                )
                .bind(self.project_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
        };

        sqlx::query(
            "INSERT INTO draft_nodes (id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(self.project_id)
        .bind(&parent_id)
        .bind(position)
        .bind(&req.name)
        .bind(req.node_type.as_str())
        .bind(&req.content)
        .bind(req.x)
        .bind(req.y)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validates relationships
        for task_id in &req.validates {
            sqlx::query(
                "INSERT INTO draft_node_validates (eval_id, task_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(&id)
            .bind(task_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships
        for blocker_id in &req.blocked_by {
            sqlx::query(
                "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(&id)
            .bind(blocker_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        self.get_draft_node(&id).await
    }

    /// Update a draft node
    ///
    /// Validation: Eval nodes cannot have validates cleared to empty
    ///
    /// Special behavior: If updating a Project (root) node's name, the project
    /// name is also updated to keep them in sync.
    pub async fn update_draft_node(
        &self,
        id: &str,
        req: &UpdateDraftNodeRequest,
    ) -> DeltaStateResult<DraftNode> {
        let pool = self.pool().await?;

        // Verify exists and get current state
        let current = self.get_draft_node(id).await?;

        // Validate: Eval nodes cannot have empty validates
        if current.node_type == NodeType::Eval {
            if let Some(ref validates) = req.validates {
                if validates.is_empty() {
                    return Err(DeltaStateError::EvalValidatesEmpty);
                }
            }
        }

        let now = utc_now();

        // Build dynamic update query
        let mut sql = String::from("UPDATE draft_nodes SET updated_at = ?");
        let mut bind_index = 2; // 1 is now

        if req.name.is_some() {
            sql.push_str(&format!(", name = ?{}", bind_index));
            bind_index += 1;
        }
        if req.content.is_some() {
            sql.push_str(&format!(", content = ?{}", bind_index));
            bind_index += 1;
        }
        if req.x.is_some() {
            sql.push_str(&format!(", x = ?{}", bind_index));
            bind_index += 1;
        }
        if req.y.is_some() {
            sql.push_str(&format!(", y = ?{}", bind_index));
            bind_index += 1;
        }

        sql.push_str(&format!(
            " WHERE id = ?{} AND project_id = ?{}",
            bind_index,
            bind_index + 1
        ));

        let mut query = sqlx::query(&sql).bind(&now);
        if let Some(ref name) = req.name {
            query = query.bind(name);
        }
        if let Some(ref content) = req.content {
            query = query.bind(content);
        }
        if let Some(x) = req.x {
            query = query.bind(x);
        }
        if let Some(y) = req.y {
            query = query.bind(y);
        }
        query = query.bind(id).bind(self.project_id);
        query.execute(pool).await?;

        // Replace validates if provided
        if let Some(ref validates) = req.validates {
            sqlx::query("DELETE FROM draft_node_validates WHERE eval_id = ? AND project_id = ?")
                .bind(id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            for task_id in validates {
                sqlx::query(
                    "INSERT INTO draft_node_validates (eval_id, task_id, project_id) VALUES (?, ?, ?)",
                )
                .bind(id)
                .bind(task_id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            }
        }

        // Replace blocked_by if provided
        if let Some(ref blocked_by) = req.blocked_by {
            sqlx::query("DELETE FROM draft_node_blocked_by WHERE node_id = ? AND project_id = ?")
                .bind(id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            for blocker_id in blocked_by {
                sqlx::query(
                    "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id) VALUES (?, ?, ?)",
                )
                .bind(id)
                .bind(blocker_id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            }
        }

        self.get_draft_node(id).await
    }

    /// Delete a draft node and all its descendants
    pub async fn delete_draft_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Verify node exists
        let _node = self.get_draft_node(id).await?;

        // Collect all descendant IDs (recursive)
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let parent_id = &to_delete[i];
            let children: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM draft_nodes WHERE parent_id = ? AND project_id = ?",
            )
            .bind(parent_id)
            .bind(self.project_id)
            .fetch_all(pool)
            .await?;
            to_delete.extend(children);
            i += 1;
        }

        // Delete all nodes and their relationships (children first due to potential FK constraints)
        for node_id in to_delete.iter().rev() {
            // Delete validates relationships (both as eval and as referenced task)
            sqlx::query("DELETE FROM draft_node_validates WHERE eval_id = ? AND project_id = ?")
                .bind(node_id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            sqlx::query("DELETE FROM draft_node_validates WHERE task_id = ? AND project_id = ?")
                .bind(node_id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            // Delete blocked_by relationships (both as blocker and as blocked)
            sqlx::query("DELETE FROM draft_node_blocked_by WHERE node_id = ? AND project_id = ?")
                .bind(node_id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            sqlx::query(
                "DELETE FROM draft_node_blocked_by WHERE blocker_id = ? AND project_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
            // Delete the node itself
            sqlx::query("DELETE FROM draft_nodes WHERE id = ? AND project_id = ?")
                .bind(node_id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
        }

        Ok(())
    }

    /// Reset tree - delete all draft nodes except the root (project node)
    ///
    /// This preserves the root node but removes all its children.
    pub async fn reset_tree(&self) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Delete all relationships (root node doesn't have validates/blocked_by)
        sqlx::query("DELETE FROM draft_node_validates WHERE project_id = ?")
            .bind(self.project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM draft_node_blocked_by WHERE project_id = ?")
            .bind(self.project_id)
            .execute(pool)
            .await?;

        // Delete all draft nodes except the root (where parent_id IS NOT NULL)
        sqlx::query("DELETE FROM draft_nodes WHERE project_id = ? AND parent_id IS NOT NULL")
            .bind(self.project_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Move a draft node to a new parent and/or position
    ///
    /// Validation:
    /// - Cannot move to null parent if a root already exists (would create second root)
    /// - If new_parent_id is provided, it must exist
    pub async fn move_draft_node(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_position: i32,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Verify node exists
        let _node = self.get_draft_node(id).await?;

        // Validate parent exists if specified
        if let Some(ref pid) = new_parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ? AND project_id = ?)",
            )
            .bind(pid)
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.to_string()));
            }
        }
        // Moving to root (parent_id = None) is allowed - multiple roots are supported

        let now = utc_now();
        sqlx::query(
            "UPDATE draft_nodes SET parent_id = ?, position = ?, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(new_parent_id)
        .bind(new_position)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Create a draft node with a specific ID (for agent import)
    ///
    /// If the ID already exists, returns the existing node (idempotent).
    /// Otherwise creates a new node with the given ID.
    pub async fn create_draft_node_with_id(
        &self,
        id: &str,
        parent_id: Option<&str>,
        name: &str,
        node_type: NodeType,
        content: &str,
        validates: &[String],
        blocked_by: &[String],
    ) -> DeltaStateResult<DraftNode> {
        // Check if already exists
        match self.get_draft_node(id).await {
            Ok(existing) => return Ok(existing),
            Err(DeltaStateError::DraftNodeNotFound(_)) => {}
            Err(e) => return Err(e),
        }

        let pool = self.pool().await?;
        let now = utc_now();

        // Validate parent exists if specified
        if let Some(pid) = parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ? AND project_id = ?)",
            )
            .bind(pid)
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.to_string()));
            }
        }

        // Get position (append to end of siblings)
        let position: i32 = {
            let max: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(position) FROM draft_nodes WHERE parent_id IS ? AND project_id = ?",
            )
            .bind(parent_id)
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;
            max.unwrap_or(-1) + 1
        };

        sqlx::query(
            "INSERT INTO draft_nodes (id, project_id, parent_id, position, name, node_type, content, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(parent_id)
        .bind(position)
        .bind(name)
        .bind(node_type.as_str())
        .bind(content)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validates relationships
        for task_id in validates {
            sqlx::query(
                "INSERT INTO draft_node_validates (eval_id, task_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(task_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships
        for blocker_id in blocked_by {
            sqlx::query(
                "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(blocker_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        self.get_draft_node(id).await
    }

    /// Get the root node ID for this project
    pub async fn get_root_node_id(&self) -> DeltaStateResult<Option<String>> {
        let pool = self.pool().await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM draft_nodes WHERE parent_id IS NULL AND project_id = ? ORDER BY position LIMIT 1",
        )
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?;
        Ok(id)
    }

    /// Get direct children of a node
    pub async fn get_children_of(&self, parent_id: &str) -> DeltaStateResult<Vec<DraftNode>> {
        let pool = self.pool().await?;

        // Load relationships
        let validates_map = self.load_draft_validates(pool).await?;
        let blocked_by_map = self.load_draft_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE parent_id = ? AND project_id = ?
             ORDER BY position",
        )
        .bind(parent_id)
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                DraftNode {
                    id: id.clone(),
                    project_id: row.get("project_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Build tree from flat draft nodes
    ///
    /// Uses recursive approach to ensure all descendants are included.
    /// blocked_by is now stored per-node, not computed from validates.
    pub fn build_draft_tree(&self, nodes: &[DraftNode]) -> Vec<DraftNodeTree> {
        // Group children by parent
        let mut children_by_parent: HashMap<Option<String>, Vec<&DraftNode>> = HashMap::new();
        for node in nodes {
            children_by_parent
                .entry(node.parent_id.clone())
                .or_default()
                .push(node);
        }

        // Sort children by position
        for children in children_by_parent.values_mut() {
            children.sort_by_key(|n| n.position);
        }

        // Recursive tree builder
        fn build_node(
            node: &DraftNode,
            children_by_parent: &HashMap<Option<String>, Vec<&DraftNode>>,
        ) -> DraftNodeTree {
            let children: Vec<DraftNodeTree> = children_by_parent
                .get(&Some(node.id.clone()))
                .map(|kids| {
                    kids.iter()
                        .map(|child| build_node(child, children_by_parent))
                        .collect()
                })
                .unwrap_or_default();

            DraftNodeTree {
                id: node.id.clone(),
                name: node.name.clone(),
                node_type: node.node_type.clone(),
                content: node.content.clone(),
                validates: node.validates.clone(),
                blocked_by: node.blocked_by.clone(),
                children,
                x: node.x,
                y: node.y,
            }
        }

        // Build from roots (nodes with no parent)
        children_by_parent
            .get(&None)
            .map(|roots| {
                roots
                    .iter()
                    .map(|root| build_node(root, &children_by_parent))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get draft tree for project
    pub async fn get_draft_tree(&self) -> DeltaStateResult<Vec<DraftNodeTree>> {
        let nodes = self.get_draft_nodes().await?;
        Ok(self.build_draft_tree(&nodes))
    }

    // =========================================================================
    // Live Node Operations
    // =========================================================================

    /// Get all live nodes as flat list
    pub async fn get_live_nodes(&self) -> DeltaStateResult<Vec<LiveNode>> {
        let pool = self.pool().await?;

        // Load relationships first
        let validates_map = self.load_live_validates(pool).await?;
        let blocked_by_map = self.load_live_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at, completed_at, last_commit_sha, claimed_by, claimed_at, completed_by, eval_result, eval_feedback, tokens_used
             FROM live_nodes
             WHERE project_id = ?
             ORDER BY parent_id NULLS FIRST, position",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                LiveNode {
                    id: id.clone(),
                    project_id: row.get("project_id"),
                    draft_node_id: row.get("draft_node_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    status: LiveNodeStatus::from_str(&row.get::<String, _>("status")),
                    source: LiveNodeSource::from_str(&row.get::<String, _>("source")),
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    completed_at: row.get("completed_at"),
                    last_commit_sha: row.get("last_commit_sha"),
                    claimed_by: row.get("claimed_by"),
                    claimed_at: row.get("claimed_at"),
                    completed_by: row.get("completed_by"),
                    eval_result: row
                        .get::<Option<String>, _>("eval_result")
                        .and_then(|s| EvalResult::from_str(&s)),
                    eval_feedback: row.get("eval_feedback"),
                    tokens_used: row.get("tokens_used"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Get a single live node
    pub async fn get_live_node(&self, id: &str) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;

        let validates = self.load_live_node_validates(pool, id).await?;
        let blocked_by = self.load_live_node_blocked_by(pool, id).await?;

        let row = sqlx::query(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at, completed_at, last_commit_sha, claimed_by, claimed_at, completed_by, eval_result, eval_feedback, tokens_used
             FROM live_nodes
             WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::LiveNodeNotFound(id.to_string()))?;

        Ok(LiveNode {
            id: row.get("id"),
            project_id: row.get("project_id"),
            draft_node_id: row.get("draft_node_id"),
            parent_id: row.get("parent_id"),
            position: row.get("position"),
            name: row.get("name"),
            node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
            content: row.get("content"),
            status: LiveNodeStatus::from_str(&row.get::<String, _>("status")),
            source: LiveNodeSource::from_str(&row.get::<String, _>("source")),
            validates,
            blocked_by,
            x: row.get("x"),
            y: row.get("y"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            completed_at: row.get("completed_at"),
            last_commit_sha: row.get("last_commit_sha"),
            claimed_by: row.get("claimed_by"),
            claimed_at: row.get("claimed_at"),
            completed_by: row.get("completed_by"),
            eval_result: row
                .get::<Option<String>, _>("eval_result")
                .and_then(|s| EvalResult::from_str(&s)),
            eval_feedback: row.get("eval_feedback"),
            tokens_used: row.get("tokens_used"),
        })
    }

    /// Create a live node from a draft node
    pub async fn create_live_node_from_draft(
        &self,
        draft: &DraftNode,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "INSERT INTO live_nodes (id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', 'spec', ?, ?, ?, ?)",
        )
        .bind(&draft.id)
        .bind(self.project_id)
        .bind(&draft.id) // draft_node_id = draft.id initially
        .bind(&draft.parent_id)
        .bind(draft.position)
        .bind(&draft.name)
        .bind(draft.node_type.as_str())
        .bind(&draft.content)
        .bind(draft.x)
        .bind(draft.y)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validates relationships (OR IGNORE handles duplicates)
        for task_id in &draft.validates {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_validates (eval_id, task_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(&draft.id)
            .bind(task_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships (OR IGNORE handles duplicates)
        for blocker_id in &draft.blocked_by {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(&draft.id)
            .bind(blocker_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        self.get_live_node(&draft.id).await
    }

    /// Create a live node added by a worker (not from draft)
    ///
    /// These nodes have no draft_node_id and source='worker'.
    /// Used when workers call add_task MCP to add tasks during execution.
    pub async fn create_live_node_from_worker(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        node_type: NodeType,
        content: &str,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Get position (append after siblings)
        let position: i32 = match parent_id {
            Some(pid) => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM live_nodes WHERE parent_id = ? AND project_id = ?",
                )
                .bind(pid)
                .bind(self.project_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
            None => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM live_nodes WHERE parent_id IS NULL AND project_id = ?",
                )
                .bind(self.project_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
        };

        sqlx::query(
            "INSERT INTO live_nodes (id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, created_at, updated_at)
             VALUES (?, ?, NULL, ?, ?, ?, ?, ?, 'pending', 'worker', ?, ?)",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(parent_id)
        .bind(position)
        .bind(name)
        .bind(node_type.as_str())
        .bind(content)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert blocked_by relationships
        if let Some(blockers) = blocked_by {
            for blocker_id in blockers {
                sqlx::query(
                    "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id) VALUES (?, ?, ?)",
                )
                .bind(id)
                .bind(blocker_id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            }
        }

        self.get_live_node(id).await
    }

    /// Add scope task as a blocker to currently claimable nodes
    ///
    /// Called after creating the scope task on first dispatch.
    /// Only blocks nodes that would be claimable right now (no existing blockers,
    /// pending, unclaimed, leaf nodes). This avoids redundant blocking edges.
    pub async fn add_scope_blocking_to_roots(&self) -> DeltaStateResult<()> {
        // Get nodes that are currently claimable (would be workable if scope didn't exist)
        let claimable = self.get_claimable_nodes().await?;

        let pool = self.pool().await?;

        // Add scope as blocker only to claimable nodes (excluding scope itself)
        for node in claimable {
            if node.id != "scope" {
                sqlx::query(
                    "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id)
                     VALUES (?, 'scope', ?)",
                )
                .bind(&node.id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
            }
        }

        Ok(())
    }

    /// Update live node status
    pub async fn update_live_node_status(
        &self,
        id: &str,
        status: LiveNodeStatus,
        commit_sha: Option<&str>,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Set completed_at on terminal statuses
        let completed_at = if matches!(
            status,
            LiveNodeStatus::Done | LiveNodeStatus::Failed | LiveNodeStatus::Validated
        ) {
            Some(now.clone())
        } else {
            None
        };

        sqlx::query(
            "UPDATE live_nodes SET status = ?, completed_at = ?, last_commit_sha = ?, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(status.as_str())
        .bind(completed_at)
        .bind(commit_sha)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        self.get_live_node(id).await
    }

    /// Update live node content (for modify deltas)
    pub async fn update_live_node_from_draft(
        &self,
        draft: &DraftNode,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE live_nodes SET name = ?, content = ?, x = ?, y = ?, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(&draft.name)
        .bind(&draft.content)
        .bind(draft.x)
        .bind(draft.y)
        .bind(&now)
        .bind(&draft.id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        // Replace validates relationships (OR IGNORE handles duplicates in list)
        sqlx::query("DELETE FROM live_node_validates WHERE eval_id = ? AND project_id = ?")
            .bind(&draft.id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        for task_id in &draft.validates {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_validates (eval_id, task_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(&draft.id)
            .bind(task_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        // Replace blocked_by relationships (OR IGNORE handles duplicates in list)
        sqlx::query("DELETE FROM live_node_blocked_by WHERE node_id = ? AND project_id = ?")
            .bind(&draft.id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        for blocker_id in &draft.blocked_by {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id) VALUES (?, ?, ?)",
            )
            .bind(&draft.id)
            .bind(blocker_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        }

        self.get_live_node(&draft.id).await
    }

    /// Delete a live node and associated submissions
    pub async fn delete_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Delete associated delta_submissions first (cascade)
        sqlx::query("DELETE FROM delta_submissions WHERE live_node_id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        // Delete validates relationships (both as eval and as referenced task)
        sqlx::query("DELETE FROM live_node_validates WHERE eval_id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM live_node_validates WHERE task_id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        // Delete blocked_by relationships (both as blocker and as blocked)
        sqlx::query("DELETE FROM live_node_blocked_by WHERE node_id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM live_node_blocked_by WHERE blocker_id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        // Delete the live node
        let result = sqlx::query("DELETE FROM live_nodes WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DeltaStateError::LiveNodeNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Build tree from flat live nodes
    ///
    /// Uses recursive approach to ensure all descendants are included.
    /// blocked_by is now stored per-node, not computed from validates.
    pub fn build_live_tree(&self, nodes: &[LiveNode]) -> Vec<LiveNodeTree> {
        // Group children by parent
        let mut children_by_parent: HashMap<Option<String>, Vec<&LiveNode>> = HashMap::new();
        for node in nodes {
            children_by_parent
                .entry(node.parent_id.clone())
                .or_default()
                .push(node);
        }

        // Sort children by position
        for children in children_by_parent.values_mut() {
            children.sort_by_key(|n| n.position);
        }

        // Recursive tree builder
        fn build_node(
            node: &LiveNode,
            children_by_parent: &HashMap<Option<String>, Vec<&LiveNode>>,
        ) -> LiveNodeTree {
            let children: Vec<LiveNodeTree> = children_by_parent
                .get(&Some(node.id.clone()))
                .map(|kids| {
                    kids.iter()
                        .map(|child| build_node(child, children_by_parent))
                        .collect()
                })
                .unwrap_or_default();

            LiveNodeTree {
                id: node.id.clone(),
                draft_node_id: node.draft_node_id.clone(),
                parent_id: node.parent_id.clone(),
                name: node.name.clone(),
                node_type: node.node_type.clone(),
                content: node.content.clone(),
                status: node.status,
                source: node.source,
                validates: node.validates.clone(),
                blocked_by: node.blocked_by.clone(),
                completed_at: node.completed_at.clone(),
                last_commit_sha: node.last_commit_sha.clone(),
                children,
                x: node.x,
                y: node.y,
                claimed_by: node.claimed_by.clone(),
                claimed_at: node.claimed_at.clone(),
                completed_by: node.completed_by.clone(),
                eval_result: node.eval_result,
                eval_feedback: node.eval_feedback.clone(),
                tokens_used: node.tokens_used,
            }
        }

        // Build from roots (nodes with no parent)
        children_by_parent
            .get(&None)
            .map(|roots| {
                roots
                    .iter()
                    .map(|root| build_node(root, &children_by_parent))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get live tree for project
    pub async fn get_live_tree(&self) -> DeltaStateResult<Vec<LiveNodeTree>> {
        let nodes = self.get_live_nodes().await?;
        Ok(self.build_live_tree(&nodes))
    }

    // =========================================================================
    // Delta Submission Operations
    // =========================================================================

    /// Create delta submissions from delta tasks
    pub async fn create_delta_submissions(
        &self,
        tasks: &[DeltaTask],
        batch_id: i64,
    ) -> DeltaStateResult<Vec<DeltaSubmission>> {
        let pool = self.pool().await?;
        let now = utc_now();
        let mut submissions = Vec::with_capacity(tasks.len());

        for task in tasks {
            let refs_json = serde_json::to_string(&task.refs)?;

            let result = sqlx::query(
                "INSERT INTO delta_submissions (project_id, batch_id, delta_type, draft_node_id, live_node_id, name, description, priority, status, refs, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
            )
            .bind(self.project_id)
            .bind(batch_id)
            .bind(task.delta_type.as_str())
            .bind(&task.draft_node_id)
            .bind(&task.live_node_id)
            .bind(&task.name)
            .bind(&task.description)
            .bind(task.priority)
            .bind(&refs_json)
            .bind(&now)
            .execute(pool)
            .await?;

            let id = result.last_insert_rowid();
            submissions.push(DeltaSubmission {
                id,
                project_id: self.project_id,
                batch_id: Some(batch_id),
                delta_type: task.delta_type,
                draft_node_id: task.draft_node_id.clone(),
                live_node_id: task.live_node_id.clone(),
                name: task.name.clone(),
                description: task.description.clone(),
                priority: task.priority,
                status: DeltaStatus::Pending,
                refs: task.refs.clone(),
                created_at: now.clone(),
                processed_at: None,
            });
        }

        Ok(submissions)
    }

    /// Get pending delta submissions for a batch
    pub async fn get_pending_submissions(
        &self,
        batch_id: i64,
    ) -> DeltaStateResult<Vec<DeltaSubmission>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, batch_id, delta_type, draft_node_id, live_node_id, name, description, priority, status, refs, created_at, processed_at
             FROM delta_submissions
             WHERE project_id = ? AND batch_id = ? AND status = 'pending'
             ORDER BY priority DESC, id",
        )
        .bind(self.project_id)
        .bind(batch_id)
        .fetch_all(pool)
        .await?;

        let submissions = rows
            .into_iter()
            .map(|row| self.row_to_delta_submission(&row))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(submissions)
    }

    /// Update delta submission status
    pub async fn update_submission_status(
        &self,
        id: i64,
        status: DeltaStatus,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        let processed_at = if status == DeltaStatus::Done || status == DeltaStatus::Failed {
            Some(now)
        } else {
            None
        };

        sqlx::query("UPDATE delta_submissions SET status = ?, processed_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(processed_at)
            .bind(id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Get next batch ID
    pub async fn next_batch_id(&self) -> DeltaStateResult<i64> {
        let pool = self.pool().await?;
        let max: Option<i64> =
            sqlx::query_scalar("SELECT MAX(batch_id) FROM delta_submissions WHERE project_id = ?")
                .bind(self.project_id)
                .fetch_optional(pool)
                .await?
                .flatten();

        Ok(max.unwrap_or(0) + 1)
    }

    fn row_to_delta_submission(
        &self,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<DeltaSubmission, DeltaStateError> {
        let submission_id: i64 = row.get("id");
        let refs_json: String = row.get("refs");
        let refs: Vec<Reference> = match serde_json::from_str(&refs_json) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(
                    "Failed to parse refs JSON for submission {}: {} - using empty",
                    submission_id,
                    e
                );
                Vec::new()
            }
        };
        let delta_type_str: String = row.get("delta_type");
        let delta_type = match DeltaType::from_str(&delta_type_str) {
            Some(dt) => dt,
            None => {
                tracing::debug!(
                    "Unknown delta_type '{}' for submission {} - using Implement",
                    delta_type_str,
                    submission_id
                );
                DeltaType::Implement
            }
        };
        let status_str: String = row.get("status");
        let status = DeltaStatus::from_str(&status_str);

        Ok(DeltaSubmission {
            id: submission_id,
            project_id: row.get("project_id"),
            batch_id: row.get("batch_id"),
            delta_type,
            draft_node_id: row.get("draft_node_id"),
            live_node_id: row.get("live_node_id"),
            name: row.get("name"),
            description: row.get("description"),
            priority: row.get("priority"),
            status,
            refs,
            created_at: row.get("created_at"),
            processed_at: row.get("processed_at"),
        })
    }

    // =========================================================================
    // Project Run Operations
    // =========================================================================

    /// Get the persistent run for this project
    pub async fn get_project_run(&self) -> DeltaStateResult<Option<ProjectRun>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, run_name, status, created_at, last_dispatch_at
             FROM project_runs
             WHERE project_id = ?",
        )
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| self.row_to_project_run(&row)))
    }

    /// Create a new persistent run for this project
    pub async fn create_project_run(&self, run_name: &str) -> DeltaStateResult<ProjectRun> {
        let pool = self.pool().await?;
        let now = utc_now();

        let result = sqlx::query(
            "INSERT INTO project_runs (project_id, run_name, status, created_at)
             VALUES (?, ?, 'paused', ?)",
        )
        .bind(self.project_id)
        .bind(run_name)
        .bind(&now)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(ProjectRun {
            id,
            project_id: self.project_id,
            run_name: run_name.to_string(),
            status: ProjectRunStatus::Paused,
            created_at: now,
            last_dispatch_at: None,
        })
    }

    /// Update project run status
    pub async fn update_project_run_status(
        &self,
        status: ProjectRunStatus,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query("UPDATE project_runs SET status = ? WHERE project_id = ?")
            .bind(status.as_str())
            .bind(self.project_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Record dispatch time
    pub async fn record_dispatch(&self) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query("UPDATE project_runs SET last_dispatch_at = ? WHERE project_id = ?")
            .bind(&now)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    fn row_to_project_run(&self, row: &sqlx::sqlite::SqliteRow) -> ProjectRun {
        let run_name: String = row.get("run_name");
        let status_str: String = row.get("status");
        let status = ProjectRunStatus::from_str(&status_str);

        ProjectRun {
            id: row.get("id"),
            project_id: row.get("project_id"),
            run_name,
            status,
            created_at: row.get("created_at"),
            last_dispatch_at: row.get("last_dispatch_at"),
        }
    }

    /// Get project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    // =========================================================================
    // Board Version Operations
    // =========================================================================

    /// Create a new board version for this project
    pub async fn create_board_version(
        &self,
        batch_id: i64,
        description: Option<&str>,
    ) -> DeltaStateResult<BoardVersion> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Get next version number for this project
        let version_number: i32 = {
            let max: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(version_number) FROM board_versions WHERE project_id = ?",
            )
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;
            max.unwrap_or(0) + 1
        };

        let result = sqlx::query(
            "INSERT INTO board_versions (project_id, batch_id, version_number, created_at, description)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(self.project_id)
        .bind(batch_id)
        .bind(version_number)
        .bind(&now)
        .bind(description)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(BoardVersion {
            id,
            project_id: self.project_id,
            batch_id,
            version_number,
            created_at: now,
            description: description.map(|s| s.to_string()),
        })
    }

    /// Get all board versions for this project
    pub async fn get_board_versions(&self) -> DeltaStateResult<Vec<BoardVersion>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE project_id = ?
             ORDER BY version_number DESC",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let versions = rows
            .into_iter()
            .map(|row| self.row_to_board_version(&row))
            .collect();

        Ok(versions)
    }

    /// Get the latest board version for this project
    pub async fn get_latest_version(&self) -> DeltaStateResult<Option<BoardVersion>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE project_id = ?
             ORDER BY version_number DESC
             LIMIT 1",
        )
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| self.row_to_board_version(&row)))
    }

    /// Get a board version by ID
    pub async fn get_board_version(&self, id: i64) -> DeltaStateResult<BoardVersion> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::DraftNodeNotFound(format!("Board version {}", id)))?;

        Ok(self.row_to_board_version(&row))
    }

    fn row_to_board_version(&self, row: &sqlx::sqlite::SqliteRow) -> BoardVersion {
        BoardVersion {
            id: row.get("id"),
            project_id: row.get("project_id"),
            batch_id: row.get("batch_id"),
            version_number: row.get("version_number"),
            created_at: row.get("created_at"),
            description: row.get("description"),
        }
    }

    // =========================================================================
    // Delivery Operations
    // =========================================================================

    /// Create a new delivery for a board version
    pub async fn create_delivery(
        &self,
        version_id: i64,
        target_branch: &str,
    ) -> DeltaStateResult<Delivery> {
        let pool = self.pool().await?;

        let result = sqlx::query(
            "INSERT INTO deliveries (project_id, version_id, status, target_branch)
             VALUES (?, ?, 'pending', ?)",
        )
        .bind(self.project_id)
        .bind(version_id)
        .bind(target_branch)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(Delivery {
            id,
            project_id: self.project_id,
            version_id,
            status: BoardDeliveryStatus::Pending,
            target_branch: target_branch.to_string(),
            delivery_branch: None,
            pr_url: None,
            pr_number: None,
            started_at: None,
            completed_at: None,
            failure_reason: None,
        })
    }

    /// Get the current (non-terminal) delivery for this project
    pub async fn get_current_delivery(&self) -> DeltaStateResult<Option<Delivery>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE project_id = ? AND status NOT IN ('merged', 'abandoned', 'failed')
             ORDER BY id DESC
             LIMIT 1",
        )
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| self.row_to_delivery(&row)))
    }

    /// Get a delivery by ID
    pub async fn get_delivery(&self, id: i64) -> DeltaStateResult<Delivery> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::DraftNodeNotFound(format!("Delivery {}", id)))?;

        Ok(self.row_to_delivery(&row))
    }

    /// Get all deliveries for a board version
    pub async fn get_deliveries_for_version(
        &self,
        version_id: i64,
    ) -> DeltaStateResult<Vec<Delivery>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE version_id = ?
             ORDER BY id DESC",
        )
        .bind(version_id)
        .fetch_all(pool)
        .await?;

        let deliveries = rows
            .into_iter()
            .map(|row| self.row_to_delivery(&row))
            .collect();

        Ok(deliveries)
    }

    /// Update delivery status
    pub async fn update_delivery_status(
        &self,
        id: i64,
        status: BoardDeliveryStatus,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Build dynamic query based on status
        let sql = if status == BoardDeliveryStatus::InProgress {
            "UPDATE deliveries SET status = ?, started_at = COALESCE(started_at, ?) WHERE id = ?"
        } else if status.is_terminal() {
            "UPDATE deliveries SET status = ?, completed_at = ? WHERE id = ?"
        } else {
            "UPDATE deliveries SET status = ? WHERE id = ?"
        };

        if status == BoardDeliveryStatus::InProgress || status.is_terminal() {
            sqlx::query(sql)
                .bind(status.as_str())
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?;
        } else {
            sqlx::query(sql)
                .bind(status.as_str())
                .bind(id)
                .execute(pool)
                .await?;
        }

        Ok(())
    }

    /// Update delivery with branch and PR info
    pub async fn update_delivery_info(
        &self,
        id: i64,
        delivery_branch: Option<&str>,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query(
            "UPDATE deliveries SET delivery_branch = ?, pr_url = ?, pr_number = ? WHERE id = ?",
        )
        .bind(delivery_branch)
        .bind(pr_url)
        .bind(pr_number)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark delivery as failed with a reason
    pub async fn fail_delivery(&self, id: i64, reason: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE deliveries SET status = 'failed', completed_at = ?, failure_reason = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    fn row_to_delivery(&self, row: &sqlx::sqlite::SqliteRow) -> Delivery {
        Delivery {
            id: row.get("id"),
            project_id: row.get("project_id"),
            version_id: row.get("version_id"),
            status: BoardDeliveryStatus::from_str(&row.get::<String, _>("status")),
            target_branch: row.get("target_branch"),
            delivery_branch: row.get("delivery_branch"),
            pr_url: row.get("pr_url"),
            pr_number: row.get("pr_number"),
            started_at: row.get("started_at"),
            completed_at: row.get("completed_at"),
            failure_reason: row.get("failure_reason"),
        }
    }

    // =========================================================================
    // Delivery Attempt Operations
    // =========================================================================

    /// Add a delivery attempt
    pub async fn add_delivery_attempt(
        &self,
        delivery_id: i64,
    ) -> DeltaStateResult<DeliveryAttempt> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Get next attempt number
        let attempt_number: i32 = {
            let max: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(attempt_number) FROM delivery_attempts WHERE delivery_id = ?",
            )
            .bind(delivery_id)
            .fetch_one(pool)
            .await?;
            max.unwrap_or(0) + 1
        };

        let result = sqlx::query(
            "INSERT INTO delivery_attempts (delivery_id, attempt_number, status, started_at)
             VALUES (?, ?, 'failed', ?)",
        )
        .bind(delivery_id)
        .bind(attempt_number)
        .bind(&now)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(DeliveryAttempt {
            id,
            delivery_id,
            attempt_number,
            status: DeliveryAttemptStatus::Failed, // Will be updated on completion
            started_at: now,
            completed_at: None,
            error_message: None,
        })
    }

    /// Complete a delivery attempt
    pub async fn complete_delivery_attempt(
        &self,
        id: i64,
        status: DeliveryAttemptStatus,
        error_message: Option<&str>,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE delivery_attempts SET status = ?, completed_at = ?, error_message = ? WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(&now)
        .bind(error_message)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get delivery attempts for a delivery
    pub async fn get_delivery_attempts(
        &self,
        delivery_id: i64,
    ) -> DeltaStateResult<Vec<DeliveryAttempt>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, delivery_id, attempt_number, status, started_at, completed_at, error_message
             FROM delivery_attempts
             WHERE delivery_id = ?
             ORDER BY attempt_number DESC",
        )
        .bind(delivery_id)
        .fetch_all(pool)
        .await?;

        let attempts = rows
            .into_iter()
            .map(|row| DeliveryAttempt {
                id: row.get("id"),
                delivery_id: row.get("delivery_id"),
                attempt_number: row.get("attempt_number"),
                status: DeliveryAttemptStatus::from_str(&row.get::<String, _>("status")),
                started_at: row.get("started_at"),
                completed_at: row.get("completed_at"),
                error_message: row.get("error_message"),
            })
            .collect();

        Ok(attempts)
    }

    /// List all deliveries in resolving_conflicts status (for daemon polling)
    pub async fn list_resolving_deliveries() -> DeltaStateResult<Vec<(i64, Delivery)>> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE status = 'resolving_conflicts'",
        )
        .fetch_all(pool)
        .await?;

        let deliveries = rows
            .into_iter()
            .map(|row| {
                let project_id: i64 = row.get("project_id");
                (
                    project_id,
                    Delivery {
                        id: row.get("id"),
                        project_id,
                        version_id: row.get("version_id"),
                        status: BoardDeliveryStatus::ResolvingConflicts,
                        target_branch: row.get("target_branch"),
                        delivery_branch: row.get("delivery_branch"),
                        pr_url: row.get("pr_url"),
                        pr_number: row.get("pr_number"),
                        started_at: row.get("started_at"),
                        completed_at: row.get("completed_at"),
                        failure_reason: row.get("failure_reason"),
                    },
                )
            })
            .collect();

        Ok(deliveries)
    }

    /// List all project runs across all projects (for run listing)
    ///
    /// Returns tuples of (ProjectRun, project_name) for building run summaries.
    pub async fn list_all_project_runs() -> DeltaStateResult<Vec<(ProjectRun, String)>> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;

        let rows = sqlx::query(
            "SELECT pr.id, pr.project_id, pr.run_name, pr.status, pr.created_at, pr.last_dispatch_at,
                    p.name as project_name
             FROM project_runs pr
             JOIN projects p ON pr.project_id = p.id
             ORDER BY pr.created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        let runs = rows
            .into_iter()
            .map(|row| {
                let status_str: String = row.get("status");
                (
                    ProjectRun {
                        id: row.get("id"),
                        project_id: row.get("project_id"),
                        run_name: row.get("run_name"),
                        status: ProjectRunStatus::from_str(&status_str),
                        created_at: row.get("created_at"),
                        last_dispatch_at: row.get("last_dispatch_at"),
                    },
                    row.get::<String, _>("project_name"),
                )
            })
            .collect();

        Ok(runs)
    }

    // =========================================================================
    // Orchestration Operations (Task Claiming, Completion, Eval)
    // =========================================================================

    /// Claim a live node for a worker
    pub async fn claim_live_node(&self, id: &str, worker_name: &str) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Verify node exists and is claimable
        let node = self.get_live_node(id).await?;
        if node.status != LiveNodeStatus::Pending {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not in pending status (current: {:?})",
                id, node.status
            )));
        }
        if node.claimed_by.is_some() {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is already claimed by {:?}",
                id, node.claimed_by
            )));
        }

        sqlx::query(
            "UPDATE live_nodes SET status = ?, claimed_by = ?, claimed_at = ?, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(LiveNodeStatus::Working.as_str())
        .bind(worker_name)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        self.get_live_node(id).await
    }

    /// Unclaim a live node (worker gives up the task)
    pub async fn unclaim_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE live_nodes SET status = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(LiveNodeStatus::Pending.as_str())
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Complete a live node (mark as done by worker)
    ///
    /// For work tasks: sets status to Done (or AwaitingEval if it has a validating eval)
    /// For eval tasks: use eval_pass or eval_fail instead
    pub async fn complete_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_live_node(id).await?;

        // Verify claimed by this worker
        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not claimed by {}",
                id, worker_name
            )));
        }

        // Determine new status based on whether there's a validating eval
        let new_status = if node.node_type == NodeType::Task && self.has_validating_eval(id).await?
        {
            LiveNodeStatus::AwaitingEval
        } else {
            LiveNodeStatus::Done
        };

        sqlx::query(
            "UPDATE live_nodes SET status = ?, completed_at = ?, completed_by = ?, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(new_status.as_str())
        .bind(&now)
        .bind(worker_name)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        // Propagate status up to parent
        self.propagate_parent_status(id).await?;

        // Check if all live nodes are complete → pause project run
        self.check_project_run_completion().await?;

        self.get_live_node(id).await
    }

    /// Check if all live nodes are complete and pause the project run if so
    async fn check_project_run_completion(&self) -> DeltaStateResult<()> {
        let nodes = self.get_live_nodes().await?;
        if nodes.is_empty() {
            return Ok(());
        }

        let all_done = nodes.iter().all(|n| {
            n.status == LiveNodeStatus::Done
                || n.status == LiveNodeStatus::Failed
                || n.status == LiveNodeStatus::Validated
        });

        if all_done {
            tracing::info!(
                "All live nodes complete for project {}, pausing run",
                self.project_id
            );
            self.update_project_run_status(ProjectRunStatus::Paused)
                .await?;
        }

        Ok(())
    }

    /// Check if a node has a validating eval
    pub async fn has_validating_eval(&self, node_id: &str) -> DeltaStateResult<bool> {
        let pool = self.pool().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM live_node_validates WHERE task_id = ? AND project_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .fetch_one(pool)
        .await?;
        Ok(count > 0)
    }

    /// Propagate status changes up to parent nodes.
    ///
    /// Called after a child's status changes. Checks if all siblings have the same
    /// "complete" status and updates parent accordingly. Recursively propagates up.
    async fn propagate_parent_status(&self, child_id: &str) -> DeltaStateResult<()> {
        let child = self.get_live_node(child_id).await?;

        // No parent = nothing to propagate
        let parent_id = match &child.parent_id {
            Some(pid) => pid.clone(),
            None => return Ok(()),
        };

        // Get all siblings (children of same parent)
        let siblings = self.get_children(&parent_id).await?;
        if siblings.is_empty() {
            return Ok(()); // Shouldn't happen, but safety check
        }

        // Check if ALL siblings have same complete status
        let all_validated = siblings
            .iter()
            .all(|s| s.status == LiveNodeStatus::Validated);
        let all_done_or_validated = siblings.iter().all(|s| {
            s.status == LiveNodeStatus::Done
                || s.status == LiveNodeStatus::AwaitingEval
                || s.status == LiveNodeStatus::Validated
        });

        // Determine new parent status
        let new_status = if all_validated {
            // All children validated - parent can be validated
            // BUT: if parent itself has a validating eval, that eval must also pass
            if self.has_validating_eval(&parent_id).await? {
                // Parent needs its own eval to pass - check if already validated
                let parent = self.get_live_node(&parent_id).await?;
                if parent.status == LiveNodeStatus::Validated {
                    // Already validated, nothing to do
                    return Ok(());
                }
                // Set to AwaitingEval - parent's eval can now run
                LiveNodeStatus::AwaitingEval
            } else {
                LiveNodeStatus::Validated
            }
        } else if all_done_or_validated {
            // All children at least "done" (work complete, maybe awaiting/validated)
            if self.has_validating_eval(&parent_id).await? {
                LiveNodeStatus::AwaitingEval
            } else {
                LiveNodeStatus::Done
            }
        } else {
            // Not all children complete - parent stays as-is
            return Ok(());
        };

        // Update parent status (only if not already at or beyond target status)
        let pool = self.pool().await?;
        let now = utc_now();
        sqlx::query(
            "UPDATE live_nodes SET status = ?, updated_at = ?
             WHERE id = ? AND project_id = ? AND status NOT IN ('done', 'awaiting_eval', 'validated')",
        )
        .bind(new_status.as_str())
        .bind(&now)
        .bind(&parent_id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        // Recursively propagate to grandparent
        Box::pin(self.propagate_parent_status(&parent_id)).await
    }

    /// Get all node IDs validated by an eval
    pub async fn get_validated_nodes(&self, eval_id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
        let node_ids: Vec<String> = sqlx::query_scalar(
            "SELECT task_id FROM live_node_validates WHERE eval_id = ? AND project_id = ?",
        )
        .bind(eval_id)
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;
        Ok(node_ids)
    }

    /// Handle eval pass - validates all nodes in the validates list
    pub async fn eval_pass(&self, eval_id: &str, worker_name: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_live_node(eval_id).await?;

        // Verify it's an eval node
        if node.node_type != NodeType::Eval {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not an eval node",
                eval_id
            )));
        }

        // Verify claimed by this worker
        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Eval '{}' is not claimed by {}",
                eval_id, worker_name
            )));
        }

        // Get validated nodes before updating eval status
        let validated_node_ids = self.get_validated_nodes(eval_id).await?;

        // Mark eval as done with pass result
        sqlx::query(
            "UPDATE live_nodes SET status = ?, completed_at = ?, completed_by = ?, eval_result = ?, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(LiveNodeStatus::Done.as_str())
        .bind(&now)
        .bind(worker_name)
        .bind(EvalResult::Pass.as_str())
        .bind(&now)
        .bind(eval_id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        // Validate all nodes in the validates list
        for node_id in &validated_node_ids {
            sqlx::query(
                "UPDATE live_nodes SET status = ?, updated_at = ? WHERE id = ? AND project_id = ? AND status IN (?, ?)",
            )
            .bind(LiveNodeStatus::Validated.as_str())
            .bind(&now)
            .bind(node_id)
            .bind(self.project_id)
            .bind(LiveNodeStatus::Done.as_str())
            .bind(LiveNodeStatus::AwaitingEval.as_str())
            .execute(pool)
            .await?;
        }

        // Propagate status up for each validated task
        for node_id in &validated_node_ids {
            self.propagate_parent_status(node_id).await?;
        }

        // Also propagate for the eval itself (in case eval is child of something)
        self.propagate_parent_status(eval_id).await?;

        // Check if all live nodes are complete → pause project run
        self.check_project_run_completion().await?;

        Ok(())
    }

    /// Handle eval fail - creates a repair node as child of the eval
    ///
    /// Returns the repair node ID
    pub async fn eval_fail(
        &self,
        eval_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> DeltaStateResult<String> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_live_node(eval_id).await?;

        // Verify it's an eval node
        if node.node_type != NodeType::Eval {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not an eval node",
                eval_id
            )));
        }

        // Verify claimed by this worker
        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Eval '{}' is not claimed by {}",
                eval_id, worker_name
            )));
        }

        // Get validated nodes before creating repair
        let validated_node_ids = self.get_validated_nodes(eval_id).await?;

        // Create repair node as child of eval
        let repair_id = format!("{}-repair-{}", eval_id, now.replace([':', '-', '.'], ""));
        let repair_name = format!("Repair: {}", feedback.chars().take(50).collect::<String>());

        sqlx::query(
            "INSERT INTO live_nodes (id, project_id, parent_id, position, name, node_type, content, status, source, created_at, updated_at)
             VALUES (?, ?, ?, 0, ?, 'task', ?, 'pending', 'system', ?, ?)",
        )
        .bind(&repair_id)
        .bind(self.project_id)
        .bind(eval_id)
        .bind(&repair_name)
        .bind(feedback)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Mark eval as pending (blocked by repair), set feedback
        // Reset claimed_by so it can be reclaimed after repair
        sqlx::query(
            "UPDATE live_nodes SET status = ?, eval_result = ?, eval_feedback = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(LiveNodeStatus::Pending.as_str())
        .bind(EvalResult::Fail.as_str())
        .bind(feedback)
        .bind(&now)
        .bind(eval_id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        // Add blocking relationship (eval is now blocked by repair node)
        sqlx::query(
            "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id) VALUES (?, ?, ?)",
        )
        .bind(eval_id)
        .bind(&repair_id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        // Mark validated nodes as needs_repair
        for node_id in &validated_node_ids {
            sqlx::query(
                "UPDATE live_nodes SET status = ?, updated_at = ? WHERE id = ? AND project_id = ? AND status IN (?, ?)",
            )
            .bind(LiveNodeStatus::NeedsRepair.as_str())
            .bind(&now)
            .bind(node_id)
            .bind(self.project_id)
            .bind(LiveNodeStatus::Done.as_str())
            .bind(LiveNodeStatus::AwaitingEval.as_str())
            .execute(pool)
            .await?;
        }

        Ok(repair_id)
    }

    /// Check if a node is blocked
    ///
    /// For work nodes: blockers must be Validated (or Done if no validating eval)
    /// For eval nodes: validated nodes must be Done/AwaitingEval/Validated
    pub async fn is_node_blocked(&self, node_id: &str) -> DeltaStateResult<bool> {
        let node = self.get_live_node(node_id).await?;

        match node.node_type {
            NodeType::Task => {
                // Check blocked_by relationships
                if node.blocked_by.is_empty() {
                    return Ok(false);
                }

                for blocker_id in &node.blocked_by {
                    // If we can't find the blocker, treat as blocked
                    // (blocker might not exist yet or was deleted)
                    let blocker = match self.get_live_node(blocker_id).await {
                        Ok(b) => b,
                        Err(_) => return Ok(true), // Can't find blocker = blocked
                    };

                    let is_blocking = if self.has_validating_eval(blocker_id).await? {
                        blocker.status != LiveNodeStatus::Validated
                    } else {
                        !blocker.status.is_complete()
                    };
                    if is_blocking {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeType::Eval => {
                // Check if validated nodes are ready
                let validates = self.get_validated_nodes(node_id).await?;
                if validates.is_empty() {
                    return Ok(true); // Eval with no validates is blocked
                }

                for task_id in &validates {
                    // If we can't find the validated task, treat as blocked
                    // (task might not exist yet or was deleted)
                    let task = match self.get_live_node(task_id).await {
                        Ok(t) => t,
                        Err(_) => return Ok(true), // Can't find task = blocked
                    };

                    let is_ready = matches!(
                        task.status,
                        LiveNodeStatus::Done
                            | LiveNodeStatus::AwaitingEval
                            | LiveNodeStatus::Validated
                    );
                    if !is_ready {
                        return Ok(true);
                    }
                }

                // Also check blocked_by (for repair flow)
                for blocker_id in &node.blocked_by {
                    // If we can't find the blocker, treat as blocked
                    let blocker = match self.get_live_node(blocker_id).await {
                        Ok(b) => b,
                        Err(_) => return Ok(true), // Can't find blocker = blocked
                    };

                    if !blocker.status.is_complete() {
                        return Ok(true);
                    }
                }

                Ok(false)
            }
        }
    }

    /// Get nodes that can be claimed (unblocked, unclaimed, pending status, no children)
    ///
    /// Priority order:
    /// 1. Eval nodes whose validated nodes are all done/awaiting_eval
    /// 2. Work nodes that are unblocked
    pub async fn get_claimable_nodes(&self) -> DeltaStateResult<Vec<LiveNode>> {
        let nodes = self.get_live_nodes().await?;

        // Build set of nodes that have children
        let nodes_with_children: std::collections::HashSet<String> =
            nodes.iter().filter_map(|n| n.parent_id.clone()).collect();

        let mut eval_nodes = vec![];
        let mut work_nodes = vec![];

        for node in nodes {
            // Must be pending and unclaimed
            if node.status != LiveNodeStatus::Pending || node.claimed_by.is_some() {
                continue;
            }

            // Skip nodes that have children - work on leaf nodes instead
            if nodes_with_children.contains(&node.id) {
                continue;
            }

            // Check if blocked
            if self.is_node_blocked(&node.id).await? {
                continue;
            }

            match node.node_type {
                NodeType::Eval => eval_nodes.push(node),
                NodeType::Task => work_nodes.push(node),
            }
        }

        // Return eval nodes first (higher priority), then work nodes
        eval_nodes.extend(work_nodes);
        Ok(eval_nodes)
    }

    /// Set tokens used on a node
    pub async fn set_node_tokens(&self, id: &str, tokens: i64) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query("UPDATE live_nodes SET tokens_used = ? WHERE id = ? AND project_id = ?")
            .bind(tokens)
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Get the node currently claimed by a worker
    pub async fn get_claimed_node_for_worker(
        &self,
        worker_name: &str,
    ) -> DeltaStateResult<Option<LiveNode>> {
        let pool = self.pool().await?;

        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM live_nodes WHERE claimed_by = ? AND project_id = ? AND status = 'working'",
        )
        .bind(worker_name)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?;

        match id {
            Some(id) => Ok(Some(self.get_live_node(&id).await?)),
            None => Ok(None),
        }
    }

    /// Reopen a completed or failed live node (reset to pending)
    pub async fn reopen_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE live_nodes SET status = 'pending', claimed_by = NULL, claimed_at = NULL, completed_at = NULL, completed_by = NULL, eval_result = NULL, eval_feedback = NULL, updated_at = ? WHERE id = ? AND project_id = ?",
        )
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get blocker node IDs for a node
    pub async fn get_blockers(&self, id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
        self.load_live_node_blocked_by(pool, id).await
    }

    /// Check if a node has children
    pub async fn has_children(&self, id: &str) -> DeltaStateResult<bool> {
        let pool = self.pool().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM live_nodes WHERE parent_id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_one(pool)
        .await?;
        Ok(count > 0)
    }

    /// Get direct children of a live node
    pub async fn get_children(&self, id: &str) -> DeltaStateResult<Vec<LiveNode>> {
        let pool = self.pool().await?;

        // Load relationships
        let validates_map = self.load_live_validates(pool).await?;
        let blocked_by_map = self.load_live_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at, completed_at, last_commit_sha, claimed_by, claimed_at, completed_by, eval_result, eval_feedback, tokens_used
             FROM live_nodes
             WHERE parent_id = ? AND project_id = ?
             ORDER BY position",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let node_id: String = row.get("id");
                LiveNode {
                    id: node_id.clone(),
                    project_id: row.get("project_id"),
                    draft_node_id: row.get("draft_node_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    status: LiveNodeStatus::from_str(&row.get::<String, _>("status")),
                    source: LiveNodeSource::from_str(&row.get::<String, _>("source")),
                    validates: validates_map.get(&node_id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&node_id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    completed_at: row.get("completed_at"),
                    last_commit_sha: row.get("last_commit_sha"),
                    claimed_by: row.get("claimed_by"),
                    claimed_at: row.get("claimed_at"),
                    completed_by: row.get("completed_by"),
                    eval_result: row
                        .get::<Option<String>, _>("eval_result")
                        .and_then(|s| EvalResult::from_str(&s)),
                    eval_feedback: row.get("eval_feedback"),
                    tokens_used: row.get("tokens_used"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Delete a live node by ID
    ///
    /// This is used for task deletion from workers - removes the node
    /// and cleans up relationships.
    pub async fn delete_live_node_by_id(&self, id: &str) -> DeltaStateResult<()> {
        self.delete_live_node(id).await
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
