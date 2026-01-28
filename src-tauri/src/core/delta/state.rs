//! Database operations for draft/live trees and delta submissions
//!
//! This module handles all SQLite operations for the delta dispatch system.

use rusqlite::{params, Connection, Row as SqliteRow};
use std::collections::HashMap;
use uuid::Uuid;

use super::types::*;
use crate::core::config::global_db_path;
use crate::core::names::slugify;

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

    /// Open database connection
    fn open_db(&self) -> DeltaStateResult<Connection> {
        let db = Connection::open(global_db_path())?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
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
    pub fn create_draft_node(&self, req: &CreateDraftNodeRequest) -> DeltaStateResult<DraftNode> {
        let db = self.open_db()?;
        let id = self.generate_slug(&db, "draft_nodes", &req.name)?;
        let now = self.now();

        // Get position (append to end of siblings)
        let position: i32 = match &req.parent_id {
            Some(parent_id) => db
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) FROM draft_nodes WHERE parent_id = ?1 AND project_id = ?2",
                    params![parent_id, self.project_id],
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
                &req.parent_id,
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
    pub fn update_draft_node(
        &self,
        id: &str,
        req: &UpdateDraftNodeRequest,
    ) -> DeltaStateResult<DraftNode> {
        let db = self.open_db()?;

        // Verify exists
        let _ = self.get_draft_node(id)?;

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

    /// Delete a draft node (and all descendants via CASCADE)
    pub fn delete_draft_node(&self, id: &str) -> DeltaStateResult<()> {
        let db = self.open_db()?;
        let deleted = db.execute(
            "DELETE FROM draft_nodes WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(DeltaStateError::DraftNodeNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Move a draft node to a new parent and/or position
    pub fn move_draft_node(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_position: i32,
    ) -> DeltaStateResult<()> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "UPDATE draft_nodes SET parent_id = ?1, position = ?2, updated_at = ?3 WHERE id = ?4 AND project_id = ?5",
            params![new_parent_id, new_position, &now, id, self.project_id],
        )?;

        Ok(())
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
