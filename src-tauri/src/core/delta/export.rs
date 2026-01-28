//! Delta tree export/import for Gyp agent access
//!
//! Exports draft tree to a single `board.json` file that Gyp can read and edit.
//! Uses baseline hash tracking to detect changes for sync.
//!
//! File format:
//! ```json
//! {
//!   "tasks": [
//!     { "id": "build-api", "name": "Build API", "content": "...", "children": [...] }
//!   ],
//!   "evals": [
//!     { "id": "api-test", "name": "API Test", "content": "...", "validates": ["build-api"] }
//!   ]
//! }
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::state::DeltaState;
use super::types::{DraftNodeTree, NodeType, UpdateDraftNodeRequest};
use crate::core::config::{global_db_path, hirsel_dir};

/// Error type for export operations
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("State error: {0}")]
    State(#[from] super::state::DeltaStateError),
}

pub type ExportResult<T> = Result<T, ExportError>;

/// Single board file that Gyp reads and edits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardFile {
    pub tasks: Vec<BoardTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evals: Vec<BoardEval>,
}

/// Task in board file (no nodeType, no x/y, no validates)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardTask {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BoardTask>,
}

/// Eval in board file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardEval {
    pub id: String,
    pub name: String,
    pub content: String,
    pub validates: Vec<String>,
}

/// Result of a sync operation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncResult {
    /// Number of changes applied
    pub changes: usize,
    /// Nodes that were added
    pub nodes_added: Vec<String>,
    /// Nodes that were updated
    pub nodes_updated: Vec<String>,
    /// Nodes that were deleted
    pub nodes_deleted: Vec<String>,
}

/// Exporter for draft tree to board.json
pub struct DeltaExporter {
    project_id: i64,
    state: DeltaState,
}

impl DeltaExporter {
    /// Create a new exporter for a project
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            state: DeltaState::new(project_id),
        }
    }

    /// Get the board directory path for this project
    pub fn board_dir(&self) -> PathBuf {
        hirsel_dir()
            .join("projects")
            .join(self.project_id.to_string())
            .join("board")
    }

    /// Ensure the board directory exists
    fn ensure_board_dir(&self) -> ExportResult<PathBuf> {
        let dir = self.board_dir();
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    /// Compute a simple hash of content for baseline comparison
    fn hash_content(&self, content: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Open database connection for baseline tracking
    fn open_db(&self) -> ExportResult<Connection> {
        let db = Connection::open(global_db_path())?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;

        // Ensure delta_file_baselines table exists
        db.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS delta_file_baselines (
                project_id INTEGER NOT NULL,
                node_slug TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (project_id, node_slug)
            );
            "#,
        )?;

        Ok(db)
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string()
    }

    // =========================================================================
    // Baseline Tracking (single "board" key)
    // =========================================================================

    /// Get the baseline hash for the board file
    fn get_baseline_hash(&self) -> ExportResult<Option<String>> {
        let db = self.open_db()?;
        let hash: Option<String> = db
            .query_row(
                "SELECT content_hash FROM delta_file_baselines WHERE project_id = ?1 AND node_slug = ?2",
                params![self.project_id, "board"],
                |row| row.get(0),
            )
            .ok();
        Ok(hash)
    }

    /// Set the baseline hash for the board file
    fn set_baseline_hash(&self, hash: &str) -> ExportResult<()> {
        let db = self.open_db()?;
        let now = self.now();
        db.execute(
            "INSERT INTO delta_file_baselines (project_id, node_slug, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(project_id, node_slug) DO UPDATE SET content_hash = ?3, updated_at = ?4",
            params![self.project_id, "board", hash, now],
        )?;
        Ok(())
    }

    /// Clean up old per-node baselines (migration from per-file system)
    fn cleanup_old_baselines(&self) -> ExportResult<()> {
        let db = self.open_db()?;
        db.execute(
            "DELETE FROM delta_file_baselines WHERE project_id = ?1 AND node_slug != 'board'",
            params![self.project_id],
        )?;
        Ok(())
    }

    // =========================================================================
    // Export
    // =========================================================================

    /// Export draft tree to single board.json and establish baseline
    pub fn export_for_agent(&mut self) -> ExportResult<PathBuf> {
        let board_dir = self.ensure_board_dir()?;
        let draft_tree = self.state.get_draft_tree()?;

        // Get flat list of all draft nodes for eval lookup
        let all_nodes = self.state.get_draft_nodes()?;
        let eval_nodes: Vec<_> = all_nodes
            .iter()
            .filter(|n| n.node_type == NodeType::Eval)
            .collect();

        // Convert root's children to BoardTasks (skip the root/project node itself)
        let tasks: Vec<BoardTask> = draft_tree
            .iter()
            .flat_map(|root| root.children.iter())
            .filter(|n| n.node_type != NodeType::Eval)
            .map(Self::tree_to_board_task)
            .collect();

        // Convert eval nodes to BoardEvals
        let evals: Vec<BoardEval> = eval_nodes
            .iter()
            .map(|e| BoardEval {
                id: e.id.clone(),
                name: e.name.clone(),
                content: e.content.clone(),
                validates: e.validates.clone(),
            })
            .collect();

        let board_file = BoardFile { tasks, evals };
        let json = serde_json::to_string_pretty(&board_file)?;
        let content_hash = self.hash_content(&json);
        let path = board_dir.join("board.json");
        std::fs::write(&path, json.as_bytes())?;

        // Save single baseline hash
        self.set_baseline_hash(&content_hash)?;

        // Clean up old per-node JSON files and baselines
        self.cleanup_old_files(&board_dir)?;
        self.cleanup_old_baselines()?;

        info!("Exported draft tree to {:?}", path);
        Ok(board_dir)
    }

    /// Convert a DraftNodeTree to a BoardTask (recursively strips nodeType, x, y, validates)
    fn tree_to_board_task(node: &DraftNodeTree) -> BoardTask {
        BoardTask {
            id: node.id.clone(),
            name: node.name.clone(),
            content: node.content.clone(),
            children: node
                .children
                .iter()
                .filter(|c| c.node_type != NodeType::Eval)
                .map(Self::tree_to_board_task)
                .collect(),
        }
    }

    /// Remove old per-node JSON files (anything other than board.json)
    fn cleanup_old_files(&self, board_dir: &PathBuf) -> ExportResult<()> {
        for entry in std::fs::read_dir(board_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false)
                && path.file_name().and_then(|f| f.to_str()) != Some("board.json")
            {
                std::fs::remove_file(&path)?;
                debug!("Removed old per-node file: {:?}", path);
            }
        }
        Ok(())
    }

    // =========================================================================
    // Import
    // =========================================================================

    /// Import changes from board.json (baseline-diff sync)
    pub fn import_from_agent(&mut self) -> ExportResult<SyncResult> {
        let board_dir = self.board_dir();
        let board_path = board_dir.join("board.json");
        if !board_path.exists() {
            return Ok(SyncResult::default());
        }

        // Read and hash
        let content = std::fs::read_to_string(&board_path)?;
        let current_hash = self.hash_content(&content);

        // Check baseline — unchanged means no work
        if let Some(baseline_hash) = self.get_baseline_hash()? {
            if baseline_hash == current_hash {
                return Ok(SyncResult::default());
            }
        }

        debug!("Importing changed board.json");

        let board_file: BoardFile = serde_json::from_str(&content)?;
        let mut result = SyncResult::default();

        // Resolve root node ID
        let root_id = match self.state.get_root_node_id()? {
            Some(id) => id,
            None => {
                warn!(
                    "No root node found for project {}, skipping import",
                    self.project_id
                );
                return Ok(result);
            }
        };

        // Import tasks recursively
        let file_task_ids: HashSet<String> = Self::collect_board_task_ids(&board_file.tasks);
        for task in &board_file.tasks {
            self.import_task_tree(task, &root_id, &mut result)?;
        }

        // Detect removed tasks: DB children of root not in file
        self.detect_removed_children(&root_id, &file_task_ids, &mut result)?;

        // Import evals
        let file_eval_ids: HashSet<String> =
            board_file.evals.iter().map(|e| e.id.clone()).collect();
        for eval in &board_file.evals {
            self.import_eval(eval, &root_id, &mut result)?;
        }

        // Detect removed evals: DB eval nodes not in file
        self.detect_removed_evals(&file_eval_ids, &mut result)?;

        // Update baseline
        self.set_baseline_hash(&current_hash)?;

        result.changes =
            result.nodes_added.len() + result.nodes_updated.len() + result.nodes_deleted.len();

        if result.changes > 0 {
            info!(
                "Imported board.json: added={}, updated={}, deleted={}",
                result.nodes_added.len(),
                result.nodes_updated.len(),
                result.nodes_deleted.len()
            );
        }

        Ok(result)
    }

    /// Recursively import a task tree, creating or updating nodes
    fn import_task_tree(
        &self,
        task: &BoardTask,
        parent_id: &str,
        result: &mut SyncResult,
    ) -> ExportResult<()> {
        match self.state.get_draft_node(&task.id) {
            Ok(existing) => {
                // Update if name or content changed
                if existing.name != task.name || existing.content != task.content {
                    self.state.update_draft_node(
                        &task.id,
                        &UpdateDraftNodeRequest {
                            name: Some(task.name.clone()),
                            content: Some(task.content.clone()),
                            ..Default::default()
                        },
                    )?;
                    result.nodes_updated.push(task.id.clone());
                }
            }
            Err(super::state::DeltaStateError::DraftNodeNotFound(_)) => {
                // Create new node with the ID from the file
                self.state.create_draft_node_with_id(
                    &task.id,
                    parent_id,
                    &task.name,
                    NodeType::Task,
                    &task.content,
                    &[],
                )?;
                result.nodes_added.push(task.id.clone());
            }
            Err(e) => return Err(e.into()),
        }

        // Recurse into children
        let child_ids: HashSet<String> = task.children.iter().map(|c| c.id.clone()).collect();
        for child in &task.children {
            self.import_task_tree(child, &task.id, result)?;
        }

        // Detect removed children of this task
        self.detect_removed_children(&task.id, &child_ids, result)?;

        Ok(())
    }

    /// Import an eval node, creating or updating
    fn import_eval(
        &self,
        eval: &BoardEval,
        root_id: &str,
        result: &mut SyncResult,
    ) -> ExportResult<()> {
        match self.state.get_draft_node(&eval.id) {
            Ok(existing) => {
                if existing.name != eval.name
                    || existing.content != eval.content
                    || existing.validates != eval.validates
                {
                    self.state.update_draft_node(
                        &eval.id,
                        &UpdateDraftNodeRequest {
                            name: Some(eval.name.clone()),
                            content: Some(eval.content.clone()),
                            validates: Some(eval.validates.clone()),
                            ..Default::default()
                        },
                    )?;
                    result.nodes_updated.push(eval.id.clone());
                }
            }
            Err(super::state::DeltaStateError::DraftNodeNotFound(_)) => {
                self.state.create_draft_node_with_id(
                    &eval.id,
                    root_id,
                    &eval.name,
                    NodeType::Eval,
                    &eval.content,
                    &eval.validates,
                )?;
                result.nodes_added.push(eval.id.clone());
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// Detect children in DB that are no longer in the file's children list, and delete them
    fn detect_removed_children(
        &self,
        parent_id: &str,
        file_child_ids: &HashSet<String>,
        result: &mut SyncResult,
    ) -> ExportResult<()> {
        let db_children = self.state.get_children_of(parent_id)?;
        for child in db_children {
            // Only remove task nodes — evals are tracked separately
            if child.node_type == NodeType::Eval {
                continue;
            }
            if !file_child_ids.contains(&child.id) {
                self.state.delete_draft_node(&child.id)?;
                result.nodes_deleted.push(child.id);
            }
        }
        Ok(())
    }

    /// Detect eval nodes in DB that are no longer in the file's evals list
    fn detect_removed_evals(
        &self,
        file_eval_ids: &HashSet<String>,
        result: &mut SyncResult,
    ) -> ExportResult<()> {
        let all_nodes = self.state.get_draft_nodes()?;
        for node in all_nodes {
            if node.node_type == NodeType::Eval && !file_eval_ids.contains(&node.id) {
                self.state.delete_draft_node(&node.id)?;
                result.nodes_deleted.push(node.id);
            }
        }
        Ok(())
    }

    /// Collect all task IDs from a list of BoardTasks (recursively)
    fn collect_board_task_ids(tasks: &[BoardTask]) -> HashSet<String> {
        let mut ids = HashSet::new();
        for task in tasks {
            ids.insert(task.id.clone());
            ids.extend(Self::collect_board_task_ids(&task.children));
        }
        ids
    }

    // =========================================================================
    // Sync
    // =========================================================================

    /// Sync file changes to database (detect changes and import)
    pub fn sync_file_changes(&mut self) -> ExportResult<SyncResult> {
        self.import_from_agent()
    }
}
