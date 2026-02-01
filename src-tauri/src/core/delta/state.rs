//! Database operations for draft/live trees and delta submissions
//!
//! This module handles all SQLite operations for the delta dispatch system.

use rusqlite::{params, Connection, Row as SqliteRow};
use std::collections::HashMap;
use std::sync::Once;

use super::types::*;
use crate::core::config::global_db_path;
use crate::core::names::slugify;

/// Schema for delta tables
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS draft_nodes (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL,
    parent_id TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    node_type TEXT NOT NULL DEFAULT 'task',
    content TEXT NOT NULL DEFAULT '',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
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
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL,
    draft_node_id TEXT,
    parent_id TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    node_type TEXT NOT NULL DEFAULT 'task',
    content TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    last_commit_sha TEXT
);

CREATE INDEX IF NOT EXISTS idx_live_nodes_project ON live_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_live_nodes_parent ON live_nodes(parent_id);

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

static SCHEMA_INIT: Once = Once::new();

fn ensure_schema(db: &Connection) -> Result<(), rusqlite::Error> {
    SCHEMA_INIT.call_once(|| {
        if let Err(e) = db.execute_batch(SCHEMA) {
            tracing::error!("Failed to initialize delta schema: {}", e);
        }
    });
    // Also try to run it in case the Once already ran but on a different db connection
    // (IF NOT EXISTS makes this safe)
    db.execute_batch(SCHEMA)?;
    Ok(())
}

/// Error type for delta state operations
#[derive(Debug, thiserror::Error)]
pub enum DeltaStateError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
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

    /// Open database connection and ensure schema exists
    fn open_db(&self) -> DeltaStateResult<Connection> {
        let db = Connection::open(global_db_path())?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        ensure_schema(&db)?;
        Ok(db)
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string()
    }

    /// Generate a unique slug ID
    fn generate_slug(&self, db: &Connection, table: &str, name: &str) -> DeltaStateResult<String> {
        let base_slug = slugify(name);
        let slug = if base_slug.is_empty() {
            "node".to_string()
        } else {
            base_slug
        };

        let mut candidate = slug.clone();
        let mut counter = 1;
        loop {
            let exists: bool = db.query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ?1 AND project_id = ?2)",
                    table
                ),
                params![&candidate, self.project_id],
                |row| row.get(0),
            )?;

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
    fn load_draft_validates(
        &self,
        db: &Connection,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let mut stmt =
            db.prepare("SELECT eval_id, task_id FROM draft_node_validates WHERE project_id = ?1")?;
        let rows = stmt.query_map([self.project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (eval_id, task_id) = row?;
            map.entry(eval_id).or_default().push(task_id);
        }
        Ok(map)
    }

    /// Load all draft blocked_by relationships (node -> blockers)
    fn load_draft_blocked_by(
        &self,
        db: &Connection,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let mut stmt = db.prepare(
            "SELECT node_id, blocker_id FROM draft_node_blocked_by WHERE project_id = ?1",
        )?;
        let rows = stmt.query_map([self.project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (node_id, blocker_id) = row?;
            map.entry(node_id).or_default().push(blocker_id);
        }
        Ok(map)
    }

    /// Load validates for a single draft node
    fn load_draft_node_validates(
        &self,
        db: &Connection,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let mut stmt = db.prepare(
            "SELECT task_id FROM draft_node_validates WHERE eval_id = ?1 AND project_id = ?2",
        )?;
        let ids = stmt
            .query_map(params![node_id, self.project_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(ids)
    }

    /// Load blocked_by for a single draft node
    fn load_draft_node_blocked_by(
        &self,
        db: &Connection,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let mut stmt = db.prepare(
            "SELECT blocker_id FROM draft_node_blocked_by WHERE node_id = ?1 AND project_id = ?2",
        )?;
        let ids = stmt
            .query_map(params![node_id, self.project_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(ids)
    }

    /// Load all live validates relationships (eval -> tasks)
    fn load_live_validates(
        &self,
        db: &Connection,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let mut stmt =
            db.prepare("SELECT eval_id, task_id FROM live_node_validates WHERE project_id = ?1")?;
        let rows = stmt.query_map([self.project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (eval_id, task_id) = row?;
            map.entry(eval_id).or_default().push(task_id);
        }
        Ok(map)
    }

    /// Load all live blocked_by relationships (node -> blockers)
    fn load_live_blocked_by(
        &self,
        db: &Connection,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let mut stmt = db.prepare(
            "SELECT node_id, blocker_id FROM live_node_blocked_by WHERE project_id = ?1",
        )?;
        let rows = stmt.query_map([self.project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (node_id, blocker_id) = row?;
            map.entry(node_id).or_default().push(blocker_id);
        }
        Ok(map)
    }

    /// Load validates for a single live node
    fn load_live_node_validates(
        &self,
        db: &Connection,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let mut stmt = db.prepare(
            "SELECT task_id FROM live_node_validates WHERE eval_id = ?1 AND project_id = ?2",
        )?;
        let ids = stmt
            .query_map(params![node_id, self.project_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(ids)
    }

    /// Load blocked_by for a single live node
    fn load_live_node_blocked_by(
        &self,
        db: &Connection,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let mut stmt = db.prepare(
            "SELECT blocker_id FROM live_node_blocked_by WHERE node_id = ?1 AND project_id = ?2",
        )?;
        let ids = stmt
            .query_map(params![node_id, self.project_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(ids)
    }

    // =========================================================================
    // Draft Node Operations
    // =========================================================================

    /// Get all draft nodes as flat list
    pub fn get_draft_nodes(&self) -> DeltaStateResult<Vec<DraftNode>> {
        let db = self.open_db()?;

        // Load relationships first
        let validates_map = self.load_draft_validates(&db)?;
        let blocked_by_map = self.load_draft_blocked_by(&db)?;

        let mut stmt = db.prepare(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE project_id = ?1
             ORDER BY parent_id NULLS FIRST, position",
        )?;

        let nodes = stmt
            .query_map([self.project_id], |row| {
                let id: String = row.get("id")?;
                Ok(DraftNode {
                    id: id.clone(),
                    project_id: row.get("project_id")?,
                    parent_id: row.get("parent_id")?,
                    position: row.get("position")?,
                    name: row.get("name")?,
                    node_type: NodeType::from_str(
                        &row.get::<_, String>("node_type").unwrap_or_default(),
                    ),
                    content: row.get("content")?,
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x")?,
                    y: row.get("y")?,
                    created_at: row.get("created_at")?,
                    updated_at: row.get("updated_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Get a single draft node
    pub fn get_draft_node(&self, id: &str) -> DeltaStateResult<DraftNode> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE id = ?1 AND project_id = ?2",
        )?;

        let validates = self.load_draft_node_validates(&db, id)?;
        let blocked_by = self.load_draft_node_blocked_by(&db, id)?;

        stmt.query_row(params![id, self.project_id], |row| {
            Ok(DraftNode {
                id: row.get("id")?,
                project_id: row.get("project_id")?,
                parent_id: row.get("parent_id")?,
                position: row.get("position")?,
                name: row.get("name")?,
                node_type: NodeType::from_str(
                    &row.get::<_, String>("node_type").unwrap_or_default(),
                ),
                content: row.get("content")?,
                validates,
                blocked_by,
                x: row.get("x")?,
                y: row.get("y")?,
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                DeltaStateError::DraftNodeNotFound(id.to_string())
            }
            e => DeltaStateError::Database(e),
        })
    }

    /// Create a new draft node
    ///
    /// Validation rules:
    /// - If parent_id is provided, it must reference an existing node
    /// - If parent_id is not provided, auto-assign to root node (if one exists)
    /// - Eval nodes must have at least one task in validates
    pub fn create_draft_node(&self, req: &CreateDraftNodeRequest) -> DeltaStateResult<DraftNode> {
        let db = self.open_db()?;

        // Validate: Eval nodes must have non-empty validates
        if req.node_type == NodeType::Eval && req.validates.is_empty() {
            return Err(DeltaStateError::EvalValidatesEmpty);
        }

        // Determine parent_id with auto-assignment to root
        let parent_id = match &req.parent_id {
            Some(pid) => {
                // Validate that parent exists
                let exists: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ?1 AND project_id = ?2)",
                    params![pid, self.project_id],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(DeltaStateError::ParentNodeNotFound(pid.clone()));
                }
                Some(pid.clone())
            }
            None => {
                // Check if a root node already exists - if so, auto-assign to it
                let root_id: Option<String> = db
                    .query_row(
                        "SELECT id FROM draft_nodes WHERE parent_id IS NULL AND project_id = ?1 ORDER BY position LIMIT 1",
                        [self.project_id],
                        |row| row.get(0),
                    )
                    .ok();
                root_id // None means this will be the first root node
            }
        };

        let id = self.generate_slug(&db, "draft_nodes", &req.name)?;
        let now = self.now();

        // Get position (append to end of siblings)
        let position: i32 = match &parent_id {
            Some(pid) => db
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) FROM draft_nodes WHERE parent_id = ?1 AND project_id = ?2",
                    params![pid, self.project_id],
                    |row| row.get(0),
                )
                .unwrap_or(-1)
                + 1,
            None => db
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) FROM draft_nodes WHERE parent_id IS NULL AND project_id = ?1",
                    [self.project_id],
                    |row| row.get(0),
                )
                .unwrap_or(-1)
                + 1,
        };

        db.execute(
            "INSERT INTO draft_nodes (id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                &id,
                self.project_id,
                &parent_id,
                position,
                &req.name,
                req.node_type.as_str(),
                &req.content,
                req.x,
                req.y,
                &now,
                &now
            ],
        )?;

        // Insert validates relationships
        for task_id in &req.validates {
            db.execute(
                "INSERT INTO draft_node_validates (eval_id, task_id, project_id) VALUES (?1, ?2, ?3)",
                params![&id, task_id, self.project_id],
            )?;
        }

        // Insert blocked_by relationships
        for blocker_id in &req.blocked_by {
            db.execute(
                "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id) VALUES (?1, ?2, ?3)",
                params![&id, blocker_id, self.project_id],
            )?;
        }

        self.get_draft_node(&id)
    }

    /// Update a draft node
    ///
    /// Validation: Eval nodes cannot have validates cleared to empty
    ///
    /// Special behavior: If updating a Project (root) node's name, the project
    /// name is also updated to keep them in sync.
    pub fn update_draft_node(
        &self,
        id: &str,
        req: &UpdateDraftNodeRequest,
    ) -> DeltaStateResult<DraftNode> {
        let db = self.open_db()?;

        // Verify exists and get current state
        let current = self.get_draft_node(id)?;

        // Validate: Eval nodes cannot have empty validates
        if current.node_type == NodeType::Eval {
            if let Some(ref validates) = req.validates {
                if validates.is_empty() {
                    return Err(DeltaStateError::EvalValidatesEmpty);
                }
            }
        }

        let now = self.now();
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(ref name) = req.name {
            updates.push(format!("name = ?{}", params.len() + 1));
            params.push(Box::new(name.clone()));
        }
        if let Some(ref content) = req.content {
            updates.push(format!("content = ?{}", params.len() + 1));
            params.push(Box::new(content.clone()));
        }
        if let Some(x) = req.x {
            updates.push(format!("x = ?{}", params.len() + 1));
            params.push(Box::new(x));
        }
        if let Some(y) = req.y {
            updates.push(format!("y = ?{}", params.len() + 1));
            params.push(Box::new(y));
        }

        params.push(Box::new(id.to_string()));
        params.push(Box::new(self.project_id));

        let sql = format!(
            "UPDATE draft_nodes SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            params.len() - 1,
            params.len()
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        db.execute(&sql, param_refs.as_slice())?;

        // Replace validates if provided
        if let Some(ref validates) = req.validates {
            db.execute(
                "DELETE FROM draft_node_validates WHERE eval_id = ?1 AND project_id = ?2",
                params![id, self.project_id],
            )?;
            for task_id in validates {
                db.execute(
                    "INSERT INTO draft_node_validates (eval_id, task_id, project_id) VALUES (?1, ?2, ?3)",
                    params![id, task_id, self.project_id],
                )?;
            }
        }

        // Replace blocked_by if provided
        if let Some(ref blocked_by) = req.blocked_by {
            db.execute(
                "DELETE FROM draft_node_blocked_by WHERE node_id = ?1 AND project_id = ?2",
                params![id, self.project_id],
            )?;
            for blocker_id in blocked_by {
                db.execute(
                    "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id) VALUES (?1, ?2, ?3)",
                    params![id, blocker_id, self.project_id],
                )?;
            }
        }

        self.get_draft_node(id)
    }

    /// Delete a draft node and all its descendants
    pub fn delete_draft_node(&self, id: &str) -> DeltaStateResult<()> {
        let db = self.open_db()?;

        // Verify node exists
        let _node = self.get_draft_node(id)?;

        // Collect all descendant IDs (recursive)
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let parent_id = &to_delete[i];
            let mut stmt =
                db.prepare("SELECT id FROM draft_nodes WHERE parent_id = ?1 AND project_id = ?2")?;
            let children: Vec<String> = stmt
                .query_map(params![parent_id, self.project_id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            to_delete.extend(children);
            i += 1;
        }

        // Delete all nodes and their relationships (children first due to potential FK constraints)
        for node_id in to_delete.iter().rev() {
            // Delete validates relationships (both as eval and as referenced task)
            db.execute(
                "DELETE FROM draft_node_validates WHERE eval_id = ?1 AND project_id = ?2",
                params![node_id, self.project_id],
            )?;
            db.execute(
                "DELETE FROM draft_node_validates WHERE task_id = ?1 AND project_id = ?2",
                params![node_id, self.project_id],
            )?;
            // Delete blocked_by relationships (both as blocker and as blocked)
            db.execute(
                "DELETE FROM draft_node_blocked_by WHERE node_id = ?1 AND project_id = ?2",
                params![node_id, self.project_id],
            )?;
            db.execute(
                "DELETE FROM draft_node_blocked_by WHERE blocker_id = ?1 AND project_id = ?2",
                params![node_id, self.project_id],
            )?;
            // Delete the node itself
            db.execute(
                "DELETE FROM draft_nodes WHERE id = ?1 AND project_id = ?2",
                params![node_id, self.project_id],
            )?;
        }

        Ok(())
    }

    /// Reset tree - delete all draft nodes except the root (project node)
    ///
    /// This preserves the root node but removes all its children.
    pub fn reset_tree(&self) -> DeltaStateResult<()> {
        let db = self.open_db()?;

        // Delete all relationships (root node doesn't have validates/blocked_by)
        db.execute(
            "DELETE FROM draft_node_validates WHERE project_id = ?1",
            [self.project_id],
        )?;
        db.execute(
            "DELETE FROM draft_node_blocked_by WHERE project_id = ?1",
            [self.project_id],
        )?;

        // Delete all draft nodes except the root (where parent_id IS NOT NULL)
        db.execute(
            "DELETE FROM draft_nodes WHERE project_id = ?1 AND parent_id IS NOT NULL",
            [self.project_id],
        )?;

        Ok(())
    }

    /// Move a draft node to a new parent and/or position
    ///
    /// Validation:
    /// - Cannot move to null parent if a root already exists (would create second root)
    /// - If new_parent_id is provided, it must exist
    pub fn move_draft_node(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_position: i32,
    ) -> DeltaStateResult<()> {
        let db = self.open_db()?;

        // Verify node exists
        let _node = self.get_draft_node(id)?;

        // Validate parent exists if specified
        if let Some(ref pid) = new_parent_id {
            let exists: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ?1 AND project_id = ?2)",
                params![pid, self.project_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.to_string()));
            }
        }
        // Moving to root (parent_id = None) is allowed - multiple roots are supported

        let now = self.now();
        db.execute(
            "UPDATE draft_nodes SET parent_id = ?1, position = ?2, updated_at = ?3 WHERE id = ?4 AND project_id = ?5",
            params![new_parent_id, new_position, &now, id, self.project_id],
        )?;

        Ok(())
    }

    /// Create a draft node with a specific ID (for agent import)
    ///
    /// If the ID already exists, returns the existing node (idempotent).
    /// Otherwise creates a new node with the given ID.
    pub fn create_draft_node_with_id(
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
        match self.get_draft_node(id) {
            Ok(existing) => return Ok(existing),
            Err(DeltaStateError::DraftNodeNotFound(_)) => {}
            Err(e) => return Err(e),
        }

        let db = self.open_db()?;
        let now = self.now();

        // Validate parent exists if specified
        if let Some(pid) = parent_id {
            let exists: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ?1 AND project_id = ?2)",
                params![pid, self.project_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.to_string()));
            }
        }

        // Get position (append to end of siblings)
        let position: i32 = db
            .query_row(
                "SELECT COALESCE(MAX(position), -1) FROM draft_nodes WHERE parent_id IS ?1 AND project_id = ?2",
                params![parent_id, self.project_id],
                |row| row.get(0),
            )
            .unwrap_or(-1)
            + 1;

        db.execute(
            "INSERT INTO draft_nodes (id, project_id, parent_id, position, name, node_type, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                self.project_id,
                parent_id,
                position,
                name,
                node_type.as_str(),
                content,
                &now,
                &now
            ],
        )?;

        // Insert validates relationships
        for task_id in validates {
            db.execute(
                "INSERT INTO draft_node_validates (eval_id, task_id, project_id) VALUES (?1, ?2, ?3)",
                params![id, task_id, self.project_id],
            )?;
        }

        // Insert blocked_by relationships
        for blocker_id in blocked_by {
            db.execute(
                "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id) VALUES (?1, ?2, ?3)",
                params![id, blocker_id, self.project_id],
            )?;
        }

        self.get_draft_node(id)
    }

    /// Get the root node ID for this project
    pub fn get_root_node_id(&self) -> DeltaStateResult<Option<String>> {
        let db = self.open_db()?;
        let id: Option<String> = db
            .query_row(
                "SELECT id FROM draft_nodes WHERE parent_id IS NULL AND project_id = ?1 ORDER BY position LIMIT 1",
                [self.project_id],
                |row| row.get(0),
            )
            .ok();
        Ok(id)
    }

    /// Get direct children of a node
    pub fn get_children_of(&self, parent_id: &str) -> DeltaStateResult<Vec<DraftNode>> {
        let db = self.open_db()?;

        // Load relationships
        let validates_map = self.load_draft_validates(&db)?;
        let blocked_by_map = self.load_draft_blocked_by(&db)?;

        let mut stmt = db.prepare(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE parent_id = ?1 AND project_id = ?2
             ORDER BY position",
        )?;

        let nodes = stmt
            .query_map(params![parent_id, self.project_id], |row| {
                let id: String = row.get("id")?;
                Ok(DraftNode {
                    id: id.clone(),
                    project_id: row.get("project_id")?,
                    parent_id: row.get("parent_id")?,
                    position: row.get("position")?,
                    name: row.get("name")?,
                    node_type: NodeType::from_str(
                        &row.get::<_, String>("node_type").unwrap_or_default(),
                    ),
                    content: row.get("content")?,
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x")?,
                    y: row.get("y")?,
                    created_at: row.get("created_at")?,
                    updated_at: row.get("updated_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

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
    pub fn get_draft_tree(&self) -> DeltaStateResult<Vec<DraftNodeTree>> {
        let nodes = self.get_draft_nodes()?;
        Ok(self.build_draft_tree(&nodes))
    }

    // =========================================================================
    // Live Node Operations
    // =========================================================================

    /// Get all live nodes as flat list
    pub fn get_live_nodes(&self) -> DeltaStateResult<Vec<LiveNode>> {
        let db = self.open_db()?;

        // Load relationships first
        let validates_map = self.load_live_validates(&db)?;
        let blocked_by_map = self.load_live_blocked_by(&db)?;

        let mut stmt = db.prepare(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, x, y, created_at, updated_at, completed_at, last_commit_sha
             FROM live_nodes
             WHERE project_id = ?1
             ORDER BY parent_id NULLS FIRST, position",
        )?;

        let nodes = stmt
            .query_map([self.project_id], |row| {
                let id: String = row.get("id")?;
                Ok(LiveNode {
                    id: id.clone(),
                    project_id: row.get("project_id")?,
                    draft_node_id: row.get("draft_node_id")?,
                    parent_id: row.get("parent_id")?,
                    position: row.get("position")?,
                    name: row.get("name")?,
                    node_type: NodeType::from_str(
                        &row.get::<_, String>("node_type").unwrap_or_default(),
                    ),
                    content: row.get("content")?,
                    status: LiveNodeStatus::from_str(
                        &row.get::<_, String>("status").unwrap_or_default(),
                    ),
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x")?,
                    y: row.get("y")?,
                    created_at: row.get("created_at")?,
                    updated_at: row.get("updated_at")?,
                    completed_at: row.get("completed_at")?,
                    last_commit_sha: row.get("last_commit_sha")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Get a single live node
    pub fn get_live_node(&self, id: &str) -> DeltaStateResult<LiveNode> {
        let db = self.open_db()?;

        let validates = self.load_live_node_validates(&db, id)?;
        let blocked_by = self.load_live_node_blocked_by(&db, id)?;

        let mut stmt = db.prepare(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, x, y, created_at, updated_at, completed_at, last_commit_sha
             FROM live_nodes
             WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            Ok(LiveNode {
                id: row.get("id")?,
                project_id: row.get("project_id")?,
                draft_node_id: row.get("draft_node_id")?,
                parent_id: row.get("parent_id")?,
                position: row.get("position")?,
                name: row.get("name")?,
                node_type: NodeType::from_str(
                    &row.get::<_, String>("node_type").unwrap_or_default(),
                ),
                content: row.get("content")?,
                status: LiveNodeStatus::from_str(
                    &row.get::<_, String>("status").unwrap_or_default(),
                ),
                validates,
                blocked_by,
                x: row.get("x")?,
                y: row.get("y")?,
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
                completed_at: row.get("completed_at")?,
                last_commit_sha: row.get("last_commit_sha")?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                DeltaStateError::LiveNodeNotFound(id.to_string())
            }
            e => DeltaStateError::Database(e),
        })
    }

    /// Create a live node from a draft node
    pub fn create_live_node_from_draft(&self, draft: &DraftNode) -> DeltaStateResult<LiveNode> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "INSERT INTO live_nodes (id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, x, y, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9, ?10, ?11, ?12)",
            params![
                &draft.id,
                self.project_id,
                &draft.id, // draft_node_id = draft.id initially
                &draft.parent_id,
                draft.position,
                &draft.name,
                draft.node_type.as_str(),
                &draft.content,
                draft.x,
                draft.y,
                &now,
                &now
            ],
        )?;

        // Insert validates relationships (OR IGNORE handles duplicates)
        for task_id in &draft.validates {
            db.execute(
                "INSERT OR IGNORE INTO live_node_validates (eval_id, task_id, project_id) VALUES (?1, ?2, ?3)",
                params![&draft.id, task_id, self.project_id],
            )?;
        }

        // Insert blocked_by relationships (OR IGNORE handles duplicates)
        for blocker_id in &draft.blocked_by {
            db.execute(
                "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id) VALUES (?1, ?2, ?3)",
                params![&draft.id, blocker_id, self.project_id],
            )?;
        }

        self.get_live_node(&draft.id)
    }

    /// Update live node status
    pub fn update_live_node_status(
        &self,
        id: &str,
        status: LiveNodeStatus,
        commit_sha: Option<&str>,
    ) -> DeltaStateResult<LiveNode> {
        let db = self.open_db()?;
        let now = self.now();

        let completed_at = if status == LiveNodeStatus::Done || status == LiveNodeStatus::Failed {
            Some(now.clone())
        } else {
            None
        };

        db.execute(
            "UPDATE live_nodes SET status = ?1, completed_at = ?2, last_commit_sha = ?3, updated_at = ?4 WHERE id = ?5 AND project_id = ?6",
            params![status.as_str(), completed_at, commit_sha, &now, id, self.project_id],
        )?;

        self.get_live_node(id)
    }

    /// Update live node content (for modify deltas)
    pub fn update_live_node_from_draft(&self, draft: &DraftNode) -> DeltaStateResult<LiveNode> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "UPDATE live_nodes SET name = ?1, content = ?2, x = ?3, y = ?4, updated_at = ?5 WHERE id = ?6 AND project_id = ?7",
            params![
                &draft.name,
                &draft.content,
                draft.x,
                draft.y,
                &now,
                &draft.id,
                self.project_id
            ],
        )?;

        // Replace validates relationships (OR IGNORE handles duplicates in list)
        db.execute(
            "DELETE FROM live_node_validates WHERE eval_id = ?1 AND project_id = ?2",
            params![&draft.id, self.project_id],
        )?;
        for task_id in &draft.validates {
            db.execute(
                "INSERT OR IGNORE INTO live_node_validates (eval_id, task_id, project_id) VALUES (?1, ?2, ?3)",
                params![&draft.id, task_id, self.project_id],
            )?;
        }

        // Replace blocked_by relationships (OR IGNORE handles duplicates in list)
        db.execute(
            "DELETE FROM live_node_blocked_by WHERE node_id = ?1 AND project_id = ?2",
            params![&draft.id, self.project_id],
        )?;
        for blocker_id in &draft.blocked_by {
            db.execute(
                "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id) VALUES (?1, ?2, ?3)",
                params![&draft.id, blocker_id, self.project_id],
            )?;
        }

        self.get_live_node(&draft.id)
    }

    /// Delete a live node and associated submissions
    pub fn delete_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let db = self.open_db()?;

        // Delete associated delta_submissions first (cascade)
        db.execute(
            "DELETE FROM delta_submissions WHERE live_node_id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;

        // Delete validates relationships (both as eval and as referenced task)
        db.execute(
            "DELETE FROM live_node_validates WHERE eval_id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        db.execute(
            "DELETE FROM live_node_validates WHERE task_id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;

        // Delete blocked_by relationships (both as blocker and as blocked)
        db.execute(
            "DELETE FROM live_node_blocked_by WHERE node_id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        db.execute(
            "DELETE FROM live_node_blocked_by WHERE blocker_id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;

        // Delete the live node
        let deleted = db.execute(
            "DELETE FROM live_nodes WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
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
                name: node.name.clone(),
                node_type: node.node_type.clone(),
                content: node.content.clone(),
                status: node.status.clone(),
                validates: node.validates.clone(),
                blocked_by: node.blocked_by.clone(),
                completed_at: node.completed_at.clone(),
                last_commit_sha: node.last_commit_sha.clone(),
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

    /// Get live tree for project
    pub fn get_live_tree(&self) -> DeltaStateResult<Vec<LiveNodeTree>> {
        let nodes = self.get_live_nodes()?;
        Ok(self.build_live_tree(&nodes))
    }

    // =========================================================================
    // Delta Submission Operations
    // =========================================================================

    /// Create delta submissions from delta tasks
    pub fn create_delta_submissions(
        &self,
        tasks: &[DeltaTask],
        batch_id: i64,
    ) -> DeltaStateResult<Vec<DeltaSubmission>> {
        let db = self.open_db()?;
        let now = self.now();
        let mut submissions = Vec::with_capacity(tasks.len());

        for task in tasks {
            let refs_json = serde_json::to_string(&task.refs)?;

            db.execute(
                "INSERT INTO delta_submissions (project_id, batch_id, delta_type, draft_node_id, live_node_id, name, description, priority, status, refs, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9, ?10)",
                params![
                    self.project_id,
                    batch_id,
                    task.delta_type.as_str(),
                    &task.draft_node_id,
                    &task.live_node_id,
                    &task.name,
                    &task.description,
                    task.priority,
                    &refs_json,
                    &now
                ],
            )?;

            let id = db.last_insert_rowid();
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
    pub fn get_pending_submissions(&self, batch_id: i64) -> DeltaStateResult<Vec<DeltaSubmission>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, batch_id, delta_type, draft_node_id, live_node_id, name, description, priority, status, refs, created_at, processed_at
             FROM delta_submissions
             WHERE project_id = ?1 AND batch_id = ?2 AND status = 'pending'
             ORDER BY priority DESC, id",
        )?;

        let submissions = stmt
            .query_map(params![self.project_id, batch_id], |row| {
                self.row_to_delta_submission(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(submissions)
    }

    /// Update delta submission status
    pub fn update_submission_status(&self, id: i64, status: DeltaStatus) -> DeltaStateResult<()> {
        let db = self.open_db()?;
        let now = self.now();

        let processed_at = if status == DeltaStatus::Done || status == DeltaStatus::Failed {
            Some(now.clone())
        } else {
            None
        };

        db.execute(
            "UPDATE delta_submissions SET status = ?1, processed_at = ?2 WHERE id = ?3",
            params![status.as_str(), processed_at, id],
        )?;

        Ok(())
    }

    /// Get next batch ID
    pub fn next_batch_id(&self) -> DeltaStateResult<i64> {
        let db = self.open_db()?;
        let max: Option<i64> = db
            .query_row(
                "SELECT MAX(batch_id) FROM delta_submissions WHERE project_id = ?1",
                [self.project_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        Ok(max.unwrap_or(0) + 1)
    }

    fn row_to_delta_submission(&self, row: &SqliteRow) -> rusqlite::Result<DeltaSubmission> {
        let refs_json: String = row.get("refs")?;
        let refs: Vec<Reference> = serde_json::from_str(&refs_json).unwrap_or_default();
        let delta_type_str: String = row.get("delta_type")?;

        Ok(DeltaSubmission {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            batch_id: row.get("batch_id")?,
            delta_type: DeltaType::from_str(&delta_type_str).unwrap_or(DeltaType::Implement),
            draft_node_id: row.get("draft_node_id")?,
            live_node_id: row.get("live_node_id")?,
            name: row.get("name")?,
            description: row.get("description")?,
            priority: row.get("priority")?,
            status: DeltaStatus::from_str(&row.get::<_, String>("status").unwrap_or_default()),
            refs,
            created_at: row.get("created_at")?,
            processed_at: row.get("processed_at")?,
        })
    }

    // =========================================================================
    // Project Run Operations
    // =========================================================================

    /// Get the persistent run for this project
    pub fn get_project_run(&self) -> DeltaStateResult<Option<ProjectRun>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, run_name, status, created_at, last_dispatch_at
             FROM project_runs
             WHERE project_id = ?1",
        )?;

        match stmt.query_row([self.project_id], |row| self.row_to_project_run(row)) {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DeltaStateError::Database(e)),
        }
    }

    /// Create a new persistent run for this project
    pub fn create_project_run(&self, run_name: &str) -> DeltaStateResult<ProjectRun> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "INSERT INTO project_runs (project_id, run_name, status, created_at)
             VALUES (?1, ?2, 'paused', ?3)",
            params![self.project_id, run_name, &now],
        )?;

        let id = db.last_insert_rowid();
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
    pub fn update_project_run_status(&self, status: ProjectRunStatus) -> DeltaStateResult<()> {
        let db = self.open_db()?;

        db.execute(
            "UPDATE project_runs SET status = ?1 WHERE project_id = ?2",
            params![status.as_str(), self.project_id],
        )?;

        Ok(())
    }

    /// Record dispatch time
    pub fn record_dispatch(&self) -> DeltaStateResult<()> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "UPDATE project_runs SET last_dispatch_at = ?1 WHERE project_id = ?2",
            params![&now, self.project_id],
        )?;

        Ok(())
    }

    fn row_to_project_run(&self, row: &SqliteRow) -> rusqlite::Result<ProjectRun> {
        Ok(ProjectRun {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            run_name: row.get("run_name")?,
            status: ProjectRunStatus::from_str(&row.get::<_, String>("status").unwrap_or_default()),
            created_at: row.get("created_at")?,
            last_dispatch_at: row.get("last_dispatch_at")?,
        })
    }

    /// Get project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    // =========================================================================
    // Board Version Operations
    // =========================================================================

    /// Create a new board version for this project
    pub fn create_board_version(
        &self,
        batch_id: i64,
        description: Option<&str>,
    ) -> DeltaStateResult<BoardVersion> {
        let db = self.open_db()?;
        let now = self.now();

        // Get next version number for this project
        let version_number: i32 = db
            .query_row(
                "SELECT COALESCE(MAX(version_number), 0) + 1 FROM board_versions WHERE project_id = ?1",
                [self.project_id],
                |row| row.get(0),
            )
            .unwrap_or(1);

        db.execute(
            "INSERT INTO board_versions (project_id, batch_id, version_number, created_at, description)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![self.project_id, batch_id, version_number, &now, description],
        )?;

        let id = db.last_insert_rowid();
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
    pub fn get_board_versions(&self) -> DeltaStateResult<Vec<BoardVersion>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE project_id = ?1
             ORDER BY version_number DESC",
        )?;

        let versions = stmt
            .query_map([self.project_id], |row| self.row_to_board_version(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(versions)
    }

    /// Get the latest board version for this project
    pub fn get_latest_version(&self) -> DeltaStateResult<Option<BoardVersion>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE project_id = ?1
             ORDER BY version_number DESC
             LIMIT 1",
        )?;

        match stmt.query_row([self.project_id], |row| self.row_to_board_version(row)) {
            Ok(version) => Ok(Some(version)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DeltaStateError::Database(e)),
        }
    }

    /// Get a board version by ID
    pub fn get_board_version(&self, id: i64) -> DeltaStateResult<BoardVersion> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            self.row_to_board_version(row)
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                DeltaStateError::DraftNodeNotFound(format!("Board version {}", id))
            }
            e => DeltaStateError::Database(e),
        })
    }

    fn row_to_board_version(&self, row: &SqliteRow) -> rusqlite::Result<BoardVersion> {
        Ok(BoardVersion {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            batch_id: row.get("batch_id")?,
            version_number: row.get("version_number")?,
            created_at: row.get("created_at")?,
            description: row.get("description")?,
        })
    }

    // =========================================================================
    // Delivery Operations
    // =========================================================================

    /// Create a new delivery for a board version
    pub fn create_delivery(
        &self,
        version_id: i64,
        target_branch: &str,
    ) -> DeltaStateResult<Delivery> {
        let db = self.open_db()?;

        db.execute(
            "INSERT INTO deliveries (project_id, version_id, status, target_branch)
             VALUES (?1, ?2, 'pending', ?3)",
            params![self.project_id, version_id, target_branch],
        )?;

        let id = db.last_insert_rowid();
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
    pub fn get_current_delivery(&self) -> DeltaStateResult<Option<Delivery>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE project_id = ?1 AND status NOT IN ('merged', 'abandoned', 'failed')
             ORDER BY id DESC
             LIMIT 1",
        )?;

        match stmt.query_row([self.project_id], |row| self.row_to_delivery(row)) {
            Ok(delivery) => Ok(Some(delivery)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DeltaStateError::Database(e)),
        }
    }

    /// Get a delivery by ID
    pub fn get_delivery(&self, id: i64) -> DeltaStateResult<Delivery> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE id = ?1",
        )?;

        stmt.query_row([id], |row| self.row_to_delivery(row))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DeltaStateError::DraftNodeNotFound(format!("Delivery {}", id))
                }
                e => DeltaStateError::Database(e),
            })
    }

    /// Get all deliveries for a board version
    pub fn get_deliveries_for_version(&self, version_id: i64) -> DeltaStateResult<Vec<Delivery>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE version_id = ?1
             ORDER BY id DESC",
        )?;

        let deliveries = stmt
            .query_map([version_id], |row| self.row_to_delivery(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(deliveries)
    }

    /// Update delivery status
    pub fn update_delivery_status(
        &self,
        id: i64,
        status: BoardDeliveryStatus,
    ) -> DeltaStateResult<()> {
        let db = self.open_db()?;
        let now = self.now();

        // Set started_at on first transition from pending
        let started_update = if status == BoardDeliveryStatus::InProgress {
            ", started_at = COALESCE(started_at, ?3)"
        } else {
            ""
        };

        // Set completed_at on terminal status
        let completed_update = if status.is_terminal() {
            ", completed_at = ?3"
        } else {
            ""
        };

        let sql = format!(
            "UPDATE deliveries SET status = ?1{}{} WHERE id = ?2",
            started_update, completed_update
        );

        if started_update.is_empty() && completed_update.is_empty() {
            db.execute(&sql, params![status.as_str(), id])?;
        } else {
            db.execute(&sql, params![status.as_str(), id, &now])?;
        }

        Ok(())
    }

    /// Update delivery with branch and PR info
    pub fn update_delivery_info(
        &self,
        id: i64,
        delivery_branch: Option<&str>,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
    ) -> DeltaStateResult<()> {
        let db = self.open_db()?;

        db.execute(
            "UPDATE deliveries SET delivery_branch = ?1, pr_url = ?2, pr_number = ?3 WHERE id = ?4",
            params![delivery_branch, pr_url, pr_number, id],
        )?;

        Ok(())
    }

    /// Mark delivery as failed with a reason
    pub fn fail_delivery(&self, id: i64, reason: &str) -> DeltaStateResult<()> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "UPDATE deliveries SET status = 'failed', completed_at = ?1, failure_reason = ?2 WHERE id = ?3",
            params![&now, reason, id],
        )?;

        Ok(())
    }

    fn row_to_delivery(&self, row: &SqliteRow) -> rusqlite::Result<Delivery> {
        Ok(Delivery {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            version_id: row.get("version_id")?,
            status: BoardDeliveryStatus::from_str(
                &row.get::<_, String>("status").unwrap_or_default(),
            ),
            target_branch: row.get("target_branch")?,
            delivery_branch: row.get("delivery_branch")?,
            pr_url: row.get("pr_url")?,
            pr_number: row.get("pr_number")?,
            started_at: row.get("started_at")?,
            completed_at: row.get("completed_at")?,
            failure_reason: row.get("failure_reason")?,
        })
    }

    // =========================================================================
    // Delivery Attempt Operations
    // =========================================================================

    /// Add a delivery attempt
    pub fn add_delivery_attempt(&self, delivery_id: i64) -> DeltaStateResult<DeliveryAttempt> {
        let db = self.open_db()?;
        let now = self.now();

        // Get next attempt number
        let attempt_number: i32 = db
            .query_row(
                "SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM delivery_attempts WHERE delivery_id = ?1",
                [delivery_id],
                |row| row.get(0),
            )
            .unwrap_or(1);

        db.execute(
            "INSERT INTO delivery_attempts (delivery_id, attempt_number, status, started_at)
             VALUES (?1, ?2, 'failed', ?3)",
            params![delivery_id, attempt_number, &now],
        )?;

        let id = db.last_insert_rowid();
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
    pub fn complete_delivery_attempt(
        &self,
        id: i64,
        status: DeliveryAttemptStatus,
        error_message: Option<&str>,
    ) -> DeltaStateResult<()> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "UPDATE delivery_attempts SET status = ?1, completed_at = ?2, error_message = ?3 WHERE id = ?4",
            params![status.as_str(), &now, error_message, id],
        )?;

        Ok(())
    }

    /// Get delivery attempts for a delivery
    pub fn get_delivery_attempts(
        &self,
        delivery_id: i64,
    ) -> DeltaStateResult<Vec<DeliveryAttempt>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, delivery_id, attempt_number, status, started_at, completed_at, error_message
             FROM delivery_attempts
             WHERE delivery_id = ?1
             ORDER BY attempt_number DESC",
        )?;

        let attempts = stmt
            .query_map([delivery_id], |row| {
                Ok(DeliveryAttempt {
                    id: row.get("id")?,
                    delivery_id: row.get("delivery_id")?,
                    attempt_number: row.get("attempt_number")?,
                    status: DeliveryAttemptStatus::from_str(
                        &row.get::<_, String>("status").unwrap_or_default(),
                    ),
                    started_at: row.get("started_at")?,
                    completed_at: row.get("completed_at")?,
                    error_message: row.get("error_message")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(attempts)
    }

    /// List all deliveries in resolving_conflicts status (for daemon polling)
    pub fn list_resolving_deliveries() -> DeltaStateResult<Vec<(i64, Delivery)>> {
        let db = rusqlite::Connection::open(global_db_path())?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;

        let mut stmt = db.prepare(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE status = 'resolving_conflicts'",
        )?;

        let deliveries = stmt
            .query_map([], |row| {
                let project_id: i64 = row.get("project_id")?;
                Ok((
                    project_id,
                    Delivery {
                        id: row.get("id")?,
                        project_id,
                        version_id: row.get("version_id")?,
                        status: BoardDeliveryStatus::ResolvingConflicts,
                        target_branch: row.get("target_branch")?,
                        delivery_branch: row.get("delivery_branch")?,
                        pr_url: row.get("pr_url")?,
                        pr_number: row.get("pr_number")?,
                        started_at: row.get("started_at")?,
                        completed_at: row.get("completed_at")?,
                        failure_reason: row.get("failure_reason")?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(deliveries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_test_db() -> i64 {
        let dir = tempdir().unwrap();
        let db_path = dir.keep().join("hirsel.db");

        // Create projects table and board tables
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch(
            r#"
            CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
            INSERT INTO projects (id, name) VALUES (1, 'Test Project');

            CREATE TABLE draft_nodes (
                id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                parent_id TEXT,
                position INTEGER NOT NULL DEFAULT 0,
                name TEXT NOT NULL,
                node_type TEXT NOT NULL DEFAULT 'task',
                content TEXT NOT NULL DEFAULT '',
                x REAL,
                y REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE draft_node_validates (
                eval_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                project_id INTEGER NOT NULL,
                PRIMARY KEY (project_id, eval_id, task_id)
            );

            CREATE TABLE draft_node_blocked_by (
                node_id TEXT NOT NULL,
                blocker_id TEXT NOT NULL,
                project_id INTEGER NOT NULL,
                PRIMARY KEY (project_id, node_id, blocker_id)
            );

            CREATE TABLE live_nodes (
                id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                draft_node_id TEXT,
                parent_id TEXT,
                position INTEGER NOT NULL DEFAULT 0,
                name TEXT NOT NULL,
                node_type TEXT NOT NULL DEFAULT 'task',
                content TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'pending',
                x REAL,
                y REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT,
                last_commit_sha TEXT
            );

            CREATE TABLE live_node_validates (
                eval_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                project_id INTEGER NOT NULL,
                PRIMARY KEY (project_id, eval_id, task_id)
            );

            CREATE TABLE live_node_blocked_by (
                node_id TEXT NOT NULL,
                blocker_id TEXT NOT NULL,
                project_id INTEGER NOT NULL,
                PRIMARY KEY (project_id, node_id, blocker_id)
            );

            CREATE TABLE delta_submissions (
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

            CREATE TABLE project_runs (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL UNIQUE,
                run_name TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'paused',
                created_at TEXT NOT NULL,
                last_dispatch_at TEXT
            );
            "#,
        )
        .unwrap();

        1 // project_id
    }

    #[test]
    fn test_draft_node_crud() {
        let _project_id = setup_test_db();
        // Note: Tests would need to point to the temp DB path
        // For now, just verify the types compile correctly
    }
}
