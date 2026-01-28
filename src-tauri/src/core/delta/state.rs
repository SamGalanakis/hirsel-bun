//! Database operations for draft/live trees and delta submissions
//!
//! This module handles all SQLite operations for the delta dispatch system.

use rusqlite::{params, Connection, Row as SqliteRow};
use std::collections::HashMap;
use std::sync::Once;
use uuid::Uuid;

use super::types::*;
use crate::core::config::global_db_path;
use crate::core::names::slugify;
use crate::core::project::{ProjectStore, UpdateProjectRequest};

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
    validates TEXT DEFAULT '[]',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_draft_nodes_project ON draft_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_draft_nodes_parent ON draft_nodes(parent_id);

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
    validates TEXT DEFAULT '[]',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    last_commit_sha TEXT
);

CREATE INDEX IF NOT EXISTS idx_live_nodes_project ON live_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_live_nodes_parent ON live_nodes(parent_id);

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
    #[error("Cannot move node to root - a root already exists")]
    CannotCreateSecondRoot,
    #[error("Cannot delete root node - delete the project instead")]
    CannotDeleteRoot,
    #[error("Project error: {0}")]
    Project(String),
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

    #[allow(dead_code)]
    fn new_id(&self) -> String {
        Uuid::new_v4().to_string()
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
    // Draft Node Operations
    // =========================================================================

    /// Get all draft nodes as flat list
    pub fn get_draft_nodes(&self) -> DeltaStateResult<Vec<DraftNode>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, parent_id, position, name, node_type, content, validates, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE project_id = ?1
             ORDER BY parent_id NULLS FIRST, position",
        )?;

        let nodes = stmt
            .query_map([self.project_id], |row| self.row_to_draft_node(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Get a single draft node
    pub fn get_draft_node(&self, id: &str) -> DeltaStateResult<DraftNode> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, parent_id, position, name, node_type, content, validates, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            self.row_to_draft_node(row)
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

        let validates_json = serde_json::to_string(&req.validates)?;

        db.execute(
            "INSERT INTO draft_nodes (id, project_id, parent_id, position, name, node_type, content, validates, x, y, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                &id,
                self.project_id,
                &parent_id,
                position,
                &req.name,
                req.node_type.as_str(),
                &req.content,
                &validates_json,
                req.x,
                req.y,
                &now,
                &now
            ],
        )?;

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

        // If updating a Project root node's name, sync with project name
        if current.node_type == NodeType::Project {
            if let Some(ref name) = req.name {
                let store =
                    ProjectStore::open().map_err(|e| DeltaStateError::Project(e.to_string()))?;
                store
                    .update_project(
                        self.project_id,
                        &UpdateProjectRequest {
                            name: Some(name.clone()),
                            ..Default::default()
                        },
                    )
                    .map_err(|e| DeltaStateError::Project(e.to_string()))?;
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
        if let Some(ref validates) = req.validates {
            updates.push(format!("validates = ?{}", params.len() + 1));
            let validates_json =
                serde_json::to_string(validates).unwrap_or_else(|_| "[]".to_string());
            params.push(Box::new(validates_json));
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

        self.get_draft_node(id)
    }

    /// Delete a draft node and all its descendants
    ///
    /// Note: Root nodes (node_type = 'project') cannot be deleted directly.
    /// Use delete_project instead.
    pub fn delete_draft_node(&self, id: &str) -> DeltaStateResult<()> {
        let db = self.open_db()?;

        // Verify node exists and check if it's a root node
        let node = self.get_draft_node(id)?;
        if node.node_type == NodeType::Project {
            return Err(DeltaStateError::CannotDeleteRoot);
        }

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

        // Delete all nodes (children first due to potential FK constraints)
        for node_id in to_delete.iter().rev() {
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

        // Get current node to check if it's currently the root
        let current_node = self.get_draft_node(id)?;

        match new_parent_id {
            Some(pid) => {
                // Validate parent exists
                let exists: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ?1 AND project_id = ?2)",
                    params![pid, self.project_id],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(DeltaStateError::ParentNodeNotFound(pid.to_string()));
                }
            }
            None => {
                // Moving to root - only allowed if this node is already root
                // or if there's no other root
                if current_node.parent_id.is_some() {
                    // This node is not currently root, check if another root exists
                    let root_exists: bool = db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE parent_id IS NULL AND project_id = ?1)",
                        [self.project_id],
                        |row| row.get(0),
                    )?;
                    if root_exists {
                        return Err(DeltaStateError::CannotCreateSecondRoot);
                    }
                }
            }
        }

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
        parent_id: &str,
        name: &str,
        node_type: NodeType,
        content: &str,
        validates: &[String],
    ) -> DeltaStateResult<DraftNode> {
        // Check if already exists
        match self.get_draft_node(id) {
            Ok(existing) => return Ok(existing),
            Err(DeltaStateError::DraftNodeNotFound(_)) => {}
            Err(e) => return Err(e),
        }

        let db = self.open_db()?;
        let now = self.now();

        // Validate parent exists
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ?1 AND project_id = ?2)",
            params![parent_id, self.project_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(DeltaStateError::ParentNodeNotFound(parent_id.to_string()));
        }

        // Get position (append to end of siblings)
        let position: i32 = db
            .query_row(
                "SELECT COALESCE(MAX(position), -1) FROM draft_nodes WHERE parent_id = ?1 AND project_id = ?2",
                params![parent_id, self.project_id],
                |row| row.get(0),
            )
            .unwrap_or(-1)
            + 1;

        let validates_json = serde_json::to_string(validates)?;

        db.execute(
            "INSERT INTO draft_nodes (id, project_id, parent_id, position, name, node_type, content, validates, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                self.project_id,
                parent_id,
                position,
                name,
                node_type.as_str(),
                content,
                &validates_json,
                &now,
                &now
            ],
        )?;

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
        let mut stmt = db.prepare(
            "SELECT id, project_id, parent_id, position, name, node_type, content, validates, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE parent_id = ?1 AND project_id = ?2
             ORDER BY position",
        )?;

        let nodes = stmt
            .query_map(params![parent_id, self.project_id], |row| {
                self.row_to_draft_node(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Build tree from flat draft nodes
    pub fn build_draft_tree(&self, nodes: &[DraftNode]) -> Vec<DraftNodeTree> {
        let mut node_map: HashMap<String, DraftNodeTree> = HashMap::new();
        let mut roots: Vec<String> = Vec::new();
        let mut parent_child_pairs: Vec<(String, String)> = Vec::new();

        // First pass: create tree wrappers and collect relationships
        for node in nodes {
            let tree_node: DraftNodeTree = node.clone().into();
            node_map.insert(node.id.clone(), tree_node);
            if let Some(parent_id) = &node.parent_id {
                parent_child_pairs.push((parent_id.clone(), node.id.clone()));
            } else {
                roots.push(node.id.clone());
            }
        }

        // Second pass: link children to parents
        for (parent_id, child_id) in parent_child_pairs {
            let child = node_map.get(&child_id).cloned();
            if let (Some(parent), Some(child)) = (node_map.get_mut(&parent_id), child) {
                parent.children.push(child);
            }
        }

        // Sort children by position
        fn sort_children(node: &mut DraftNodeTree, nodes: &[DraftNode]) {
            let get_pos = |id: &str| {
                nodes
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| n.position)
                    .unwrap_or(0)
            };
            node.children.sort_by_key(|n| get_pos(&n.id));
            for child in &mut node.children {
                sort_children(child, nodes);
            }
        }

        // Build result from roots
        let mut result: Vec<DraftNodeTree> = roots
            .into_iter()
            .filter_map(|id| node_map.remove(&id))
            .collect();

        // Sort roots by position
        let get_root_pos = |id: &str| {
            nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.position)
                .unwrap_or(0)
        };
        result.sort_by_key(|n| get_root_pos(&n.id));

        for root in &mut result {
            sort_children(root, nodes);
        }

        result
    }

    /// Get draft tree for project
    pub fn get_draft_tree(&self) -> DeltaStateResult<Vec<DraftNodeTree>> {
        let nodes = self.get_draft_nodes()?;
        Ok(self.build_draft_tree(&nodes))
    }

    fn row_to_draft_node(&self, row: &SqliteRow) -> rusqlite::Result<DraftNode> {
        let validates_json: String = row.get("validates")?;
        let validates: Vec<String> = serde_json::from_str(&validates_json).unwrap_or_default();

        Ok(DraftNode {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            parent_id: row.get("parent_id")?,
            position: row.get("position")?,
            name: row.get("name")?,
            node_type: NodeType::from_str(&row.get::<_, String>("node_type").unwrap_or_default()),
            content: row.get("content")?,
            validates,
            x: row.get("x")?,
            y: row.get("y")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    // =========================================================================
    // Live Node Operations
    // =========================================================================

    /// Get all live nodes as flat list
    pub fn get_live_nodes(&self) -> DeltaStateResult<Vec<LiveNode>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, validates, x, y, created_at, updated_at, completed_at, last_commit_sha
             FROM live_nodes
             WHERE project_id = ?1
             ORDER BY parent_id NULLS FIRST, position",
        )?;

        let nodes = stmt
            .query_map([self.project_id], |row| self.row_to_live_node(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Get a single live node
    pub fn get_live_node(&self, id: &str) -> DeltaStateResult<LiveNode> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, validates, x, y, created_at, updated_at, completed_at, last_commit_sha
             FROM live_nodes
             WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            self.row_to_live_node(row)
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
        let validates_json = serde_json::to_string(&draft.validates)?;

        db.execute(
            "INSERT INTO live_nodes (id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, validates, x, y, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9, ?10, ?11, ?12, ?13)",
            params![
                &draft.id,
                self.project_id,
                &draft.id, // draft_node_id = draft.id initially
                &draft.parent_id,
                draft.position,
                &draft.name,
                draft.node_type.as_str(),
                &draft.content,
                &validates_json,
                draft.x,
                draft.y,
                &now,
                &now
            ],
        )?;

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
        let validates_json = serde_json::to_string(&draft.validates)?;

        db.execute(
            "UPDATE live_nodes SET name = ?1, content = ?2, validates = ?3, x = ?4, y = ?5, updated_at = ?6 WHERE id = ?7 AND project_id = ?8",
            params![
                &draft.name,
                &draft.content,
                &validates_json,
                draft.x,
                draft.y,
                &now,
                &draft.id,
                self.project_id
            ],
        )?;

        self.get_live_node(&draft.id)
    }

    /// Delete a live node
    pub fn delete_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let db = self.open_db()?;
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
    pub fn build_live_tree(&self, nodes: &[LiveNode]) -> Vec<LiveNodeTree> {
        let mut node_map: HashMap<String, LiveNodeTree> = HashMap::new();
        let mut roots: Vec<String> = Vec::new();
        let mut parent_child_pairs: Vec<(String, String)> = Vec::new();

        for node in nodes {
            let tree_node: LiveNodeTree = node.clone().into();
            node_map.insert(node.id.clone(), tree_node);
            if let Some(parent_id) = &node.parent_id {
                parent_child_pairs.push((parent_id.clone(), node.id.clone()));
            } else {
                roots.push(node.id.clone());
            }
        }

        for (parent_id, child_id) in parent_child_pairs {
            let child = node_map.get(&child_id).cloned();
            if let (Some(parent), Some(child)) = (node_map.get_mut(&parent_id), child) {
                parent.children.push(child);
            }
        }

        fn sort_children(node: &mut LiveNodeTree, nodes: &[LiveNode]) {
            let get_pos = |id: &str| {
                nodes
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| n.position)
                    .unwrap_or(0)
            };
            node.children.sort_by_key(|n| get_pos(&n.id));
            for child in &mut node.children {
                sort_children(child, nodes);
            }
        }

        let mut result: Vec<LiveNodeTree> = roots
            .into_iter()
            .filter_map(|id| node_map.remove(&id))
            .collect();

        let get_root_pos = |id: &str| {
            nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.position)
                .unwrap_or(0)
        };
        result.sort_by_key(|n| get_root_pos(&n.id));

        for root in &mut result {
            sort_children(root, nodes);
        }

        result
    }

    /// Get live tree for project
    pub fn get_live_tree(&self) -> DeltaStateResult<Vec<LiveNodeTree>> {
        let nodes = self.get_live_nodes()?;
        Ok(self.build_live_tree(&nodes))
    }

    fn row_to_live_node(&self, row: &SqliteRow) -> rusqlite::Result<LiveNode> {
        let validates_json: String = row.get("validates")?;
        let validates: Vec<String> = serde_json::from_str(&validates_json).unwrap_or_default();

        Ok(LiveNode {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            draft_node_id: row.get("draft_node_id")?,
            parent_id: row.get("parent_id")?,
            position: row.get("position")?,
            name: row.get("name")?,
            node_type: NodeType::from_str(&row.get::<_, String>("node_type").unwrap_or_default()),
            content: row.get("content")?,
            status: LiveNodeStatus::from_str(&row.get::<_, String>("status").unwrap_or_default()),
            validates,
            x: row.get("x")?,
            y: row.get("y")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            completed_at: row.get("completed_at")?,
            last_commit_sha: row.get("last_commit_sha")?,
        })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_test_db() -> i64 {
        let dir = tempdir().unwrap();
        let db_path = dir.into_path().join("hirsel.db");

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
                validates TEXT DEFAULT '[]',
                x REAL,
                y REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
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
                validates TEXT DEFAULT '[]',
                x REAL,
                y REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT,
                last_commit_sha TEXT
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
