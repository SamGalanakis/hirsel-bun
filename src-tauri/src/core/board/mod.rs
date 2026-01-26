//! Board Service - unified interface for SpecFlow board operations
//!
//! This module provides the BoardService which handles:
//! - Database operations for tasks and evals
//! - JSON file export/import for AI agents (Gyp)
//! - Validation computation
//!
//! ## Data Model
//!
//! - **Tasks**: Nested tree of work items with slug IDs
//! - **Evals**: Flat list of verifications that reference tasks
//!
//! ## Agent Access
//!
//! Gyp reads/writes a single JSON file at:
//! `~/.hirsel/projects/{project_id}/board/board.json`

mod types;

pub use types::{
    BoardJson, Bookmark, CreateEvalRequest, CreateTaskRequest, Eval, EvalStatus, SyncResult, Task,
    TaskStatus, TaskTree, UpdateEvalRequest, UpdateTaskRequest,
};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use rusqlite::{params, Connection, Row as SqliteRow};
use tracing::{debug, info};

use crate::core::config::{global_db_path, hirsel_dir, OrchestratorMode, OrchestratorProfile};
use crate::core::http_client::AuthenticatedClient;
use crate::core::names::slugify;

/// Schema for board tables
const SCHEMA: &str = r#"
-- Tasks table (nested tree structure)
CREATE TABLE IF NOT EXISTS board_tasks (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES board_tasks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'todo',
    content TEXT NOT NULL DEFAULT '',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Evals table (flat list, references tasks via validates JSON array)
CREATE TABLE IF NOT EXISTS board_evals (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'blocked',
    content TEXT NOT NULL DEFAULT '',
    validates TEXT NOT NULL DEFAULT '[]',  -- JSON array of task IDs
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Saved viewport positions (bookmarks)
CREATE TABLE IF NOT EXISTS board_bookmarks (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    zoom REAL NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_board_tasks_project ON board_tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_board_tasks_parent ON board_tasks(parent_id);
CREATE INDEX IF NOT EXISTS idx_board_evals_project ON board_evals(project_id);
CREATE INDEX IF NOT EXISTS idx_board_bookmarks_project ON board_bookmarks(project_id);
"#;

/// Error type for board operations
#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] crate::core::http_client::HttpError),
    #[error("Project not found: {0}")]
    ProjectNotFound(i64),
    #[error("Task not found: {0}")]
    TaskNotFound(String),
    #[error("Eval not found: {0}")]
    EvalNotFound(String),
    #[error("Bookmark not found: {0}")]
    BookmarkNotFound(String),
    #[error("Remote error: {0}")]
    Remote(String),
}

pub type BoardResult<T> = Result<T, BoardError>;

/// Service for managing SpecFlow board data
pub struct BoardService {
    project_id: i64,
    profile: Option<OrchestratorProfile>,
    last_sync_time: Option<SystemTime>,
    baseline_hash: Option<String>,
}

impl BoardService {
    /// Create a new board service for local mode
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            profile: None,
            last_sync_time: None,
            baseline_hash: None,
        }
    }

    /// Create a board service with a profile (for remote mode)
    pub fn with_profile(project_id: i64, profile: OrchestratorProfile) -> Self {
        Self {
            project_id,
            profile: Some(profile),
            last_sync_time: None,
            baseline_hash: None,
        }
    }

    /// Get the board directory path for this project
    pub fn board_dir(&self) -> PathBuf {
        hirsel_dir()
            .join("projects")
            .join(self.project_id.to_string())
            .join("board")
    }

    /// Get the path to board.json
    fn board_json_path(&self) -> PathBuf {
        self.board_dir().join("board.json")
    }

    /// Ensure the board directory exists
    fn ensure_board_dir(&self) -> BoardResult<PathBuf> {
        let dir = self.board_dir();
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    /// Check if we should use remote mode
    fn should_use_remote(&self) -> bool {
        self.profile
            .as_ref()
            .map(|p| p.mode == OrchestratorMode::Remote && p.url.is_some())
            .unwrap_or(false)
    }

    /// Get authenticated HTTP client for remote mode
    fn remote_client(&self) -> BoardResult<AuthenticatedClient> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| BoardError::Remote("No profile configured".into()))?;

        let url = profile
            .url
            .as_ref()
            .ok_or_else(|| BoardError::Remote("No remote URL configured".into()))?;

        let api_key = profile
            .api_key
            .as_ref()
            .ok_or_else(|| BoardError::Remote("No API key configured".into()))?;

        Ok(AuthenticatedClient::new(url, api_key))
    }

    /// Open database connection
    fn open_db(&self) -> BoardResult<Connection> {
        let db = Connection::open(global_db_path())?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.execute_batch(SCHEMA)?;
        Ok(db)
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string()
    }

    /// Compute a simple hash of content for baseline comparison
    fn hash_content(&self, content: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Generate a unique slug ID from a name
    fn generate_slug(&self, db: &Connection, table: &str, name: &str) -> BoardResult<String> {
        let base_slug = slugify(name);
        let slug = if base_slug.is_empty() {
            "item".to_string()
        } else {
            base_slug
        };

        // Check for collisions
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

    // ========== TASK CRUD OPERATIONS ==========

    /// Get all tasks for a project as flat list
    pub fn get_tasks(&self) -> BoardResult<Vec<Task>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, parent_id, position, name, status, content, x, y, created_at, updated_at
             FROM board_tasks
             WHERE project_id = ?1
             ORDER BY parent_id NULLS FIRST, position",
        )?;

        let tasks = stmt
            .query_map([self.project_id], |row| self.row_to_task(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tasks)
    }

    /// Build tree structure from flat task list
    pub fn build_task_tree(&self, tasks: &[Task]) -> Vec<TaskTree> {
        let mut task_map: HashMap<String, TaskTree> = HashMap::new();
        let mut roots: Vec<String> = Vec::new();
        let mut parent_child_pairs: Vec<(String, String)> = Vec::new();

        // First pass: create TaskTree wrappers and collect parent-child relationships
        for task in tasks {
            let tree_node: TaskTree = task.clone().into();
            task_map.insert(task.id.clone(), tree_node);
            if let Some(parent_id) = &task.parent_id {
                parent_child_pairs.push((parent_id.clone(), task.id.clone()));
            } else {
                roots.push(task.id.clone());
            }
        }

        // Second pass: link children to parents
        for (parent_id, child_id) in parent_child_pairs {
            let child = task_map.get(&child_id).cloned();
            if let (Some(parent), Some(child)) = (task_map.get_mut(&parent_id), child) {
                parent.children.push(child);
            }
        }

        // Sort children by position (using original tasks for position info)
        fn sort_children(node: &mut TaskTree, tasks: &[Task]) {
            let get_pos = |id: &str| {
                tasks
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.position)
                    .unwrap_or(0)
            };
            node.children.sort_by_key(|n| get_pos(&n.id));
            for child in &mut node.children {
                sort_children(child, tasks);
            }
        }

        // Build result from roots
        let mut result: Vec<TaskTree> = roots
            .into_iter()
            .filter_map(|id| task_map.remove(&id))
            .collect();

        // Sort roots by position
        let get_root_pos = |id: &str| {
            tasks
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.position)
                .unwrap_or(0)
        };
        result.sort_by_key(|n| get_root_pos(&n.id));

        for root in &mut result {
            sort_children(root, tasks);
        }

        result
    }

    /// Get task tree for a project (with validation computed)
    pub fn get_task_tree(&self) -> BoardResult<Vec<TaskTree>> {
        let tasks = self.get_tasks()?;
        let evals = self.get_evals()?;
        let mut tree = self.build_task_tree(&tasks);

        // Compute validation
        let validation = self.compute_validation(&tree, &evals);

        // Apply validation to tree
        fn apply_validation(node: &mut TaskTree, validation: &HashMap<String, bool>) {
            node.validated = validation.get(&node.id).copied();
            for child in &mut node.children {
                apply_validation(child, validation);
            }
        }

        for root in &mut tree {
            apply_validation(root, &validation);
        }

        Ok(tree)
    }

    /// Create a new task
    pub fn create_task(&self, req: &CreateTaskRequest) -> BoardResult<Task> {
        let db = self.open_db()?;
        let id = self.generate_slug(&db, "board_tasks", &req.name)?;
        let now = self.now();

        // Get position (append to end of siblings)
        let position: i32 = match &req.parent_id {
            Some(parent_id) => db
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) FROM board_tasks WHERE parent_id = ?1 AND project_id = ?2",
                    params![parent_id, self.project_id],
                    |row| row.get(0),
                )
                .unwrap_or(-1)
                + 1,
            None => db
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) FROM board_tasks WHERE parent_id IS NULL AND project_id = ?1",
                    [self.project_id],
                    |row| row.get(0),
                )
                .unwrap_or(-1)
                + 1,
        };

        db.execute(
            "INSERT INTO board_tasks (id, project_id, parent_id, position, name, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &id,
                self.project_id,
                &req.parent_id,
                position,
                &req.name,
                &req.content,
                &now,
                &now
            ],
        )?;

        self.get_task(&id)
    }

    /// Get a task by ID
    pub fn get_task(&self, id: &str) -> BoardResult<Task> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, parent_id, position, name, status, content, x, y, created_at, updated_at
             FROM board_tasks
             WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| self.row_to_task(row))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => BoardError::TaskNotFound(id.to_string()),
                e => BoardError::Database(e),
            })
    }

    /// Update a task
    pub fn update_task(&self, id: &str, req: &UpdateTaskRequest) -> BoardResult<Task> {
        let db = self.open_db()?;

        // Verify exists
        let _ = self.get_task(id)?;

        let now = self.now();
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(ref name) = req.name {
            updates.push(format!("name = ?{}", params.len() + 1));
            params.push(Box::new(name.clone()));
        }
        if let Some(status) = req.status {
            updates.push(format!("status = ?{}", params.len() + 1));
            params.push(Box::new(status.as_str().to_string()));
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
            "UPDATE board_tasks SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            params.len() - 1,
            params.len()
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        db.execute(&sql, param_refs.as_slice())?;

        self.get_task(id)
    }

    /// Delete a task (and all descendants via CASCADE)
    pub fn delete_task(&self, id: &str) -> BoardResult<()> {
        let db = self.open_db()?;
        let deleted = db.execute(
            "DELETE FROM board_tasks WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(BoardError::TaskNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Move a task to a new parent and/or position
    pub fn move_task(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_position: i32,
    ) -> BoardResult<()> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "UPDATE board_tasks SET parent_id = ?1, position = ?2, updated_at = ?3 WHERE id = ?4 AND project_id = ?5",
            params![new_parent_id, new_position, &now, id, self.project_id],
        )?;

        Ok(())
    }

    fn row_to_task(&self, row: &SqliteRow) -> rusqlite::Result<Task> {
        Ok(Task {
            id: row.get("id")?,
            parent_id: row.get("parent_id")?,
            position: row.get("position")?,
            name: row.get("name")?,
            status: TaskStatus::from_str(&row.get::<_, String>("status").unwrap_or_default()),
            content: row.get("content")?,
            x: row.get("x")?,
            y: row.get("y")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    // ========== EVAL CRUD OPERATIONS ==========

    /// Get all evals for a project
    pub fn get_evals(&self) -> BoardResult<Vec<Eval>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, name, status, content, validates, x, y, created_at, updated_at
             FROM board_evals
             WHERE project_id = ?1
             ORDER BY created_at",
        )?;

        let evals = stmt
            .query_map([self.project_id], |row| self.row_to_eval(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(evals)
    }

    /// Create a new eval
    pub fn create_eval(&self, req: &CreateEvalRequest) -> BoardResult<Eval> {
        let db = self.open_db()?;
        let id = self.generate_slug(&db, "board_evals", &req.name)?;
        let now = self.now();
        let validates_json = serde_json::to_string(&req.validates)?;

        db.execute(
            "INSERT INTO board_evals (id, project_id, name, content, validates, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &id,
                self.project_id,
                &req.name,
                &req.content,
                &validates_json,
                &now,
                &now
            ],
        )?;

        self.get_eval(&id)
    }

    /// Get an eval by ID
    pub fn get_eval(&self, id: &str) -> BoardResult<Eval> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, name, status, content, validates, x, y, created_at, updated_at
             FROM board_evals
             WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| self.row_to_eval(row))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => BoardError::EvalNotFound(id.to_string()),
                e => BoardError::Database(e),
            })
    }

    /// Update an eval
    pub fn update_eval(&self, id: &str, req: &UpdateEvalRequest) -> BoardResult<Eval> {
        let db = self.open_db()?;

        // Verify exists
        let _ = self.get_eval(id)?;

        let now = self.now();
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(ref name) = req.name {
            updates.push(format!("name = ?{}", params.len() + 1));
            params.push(Box::new(name.clone()));
        }
        if let Some(status) = req.status {
            updates.push(format!("status = ?{}", params.len() + 1));
            params.push(Box::new(status.as_str().to_string()));
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
            "UPDATE board_evals SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            params.len() - 1,
            params.len()
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        db.execute(&sql, param_refs.as_slice())?;

        self.get_eval(id)
    }

    /// Delete an eval
    pub fn delete_eval(&self, id: &str) -> BoardResult<()> {
        let db = self.open_db()?;
        let deleted = db.execute(
            "DELETE FROM board_evals WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(BoardError::EvalNotFound(id.to_string()));
        }
        Ok(())
    }

    fn row_to_eval(&self, row: &SqliteRow) -> rusqlite::Result<Eval> {
        let validates_json: String = row.get("validates")?;
        let validates: Vec<String> = serde_json::from_str(&validates_json).unwrap_or_default();

        Ok(Eval {
            id: row.get("id")?,
            name: row.get("name")?,
            status: EvalStatus::from_str(&row.get::<_, String>("status").unwrap_or_default()),
            content: row.get("content")?,
            validates,
            x: row.get("x")?,
            y: row.get("y")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    // ========== VALIDATION COMPUTATION ==========

    /// Compute validation status for all tasks
    ///
    /// A task is validated if:
    /// 1. It has at least one eval with status=passed that references it, OR
    /// 2. All of its children are validated (and it has children)
    pub fn compute_validation(&self, tasks: &[TaskTree], evals: &[Eval]) -> HashMap<String, bool> {
        let mut validation: HashMap<String, bool> = HashMap::new();

        // Collect task IDs that have passing evals
        let mut directly_validated: HashSet<String> = HashSet::new();
        for eval in evals {
            if eval.status == EvalStatus::Passed {
                for task_id in &eval.validates {
                    directly_validated.insert(task_id.clone());
                }
            }
        }

        // Recursively compute validation
        fn compute_node_validation(
            node: &TaskTree,
            directly_validated: &HashSet<String>,
            validation: &mut HashMap<String, bool>,
        ) -> bool {
            // First, compute children's validation
            let children_validated = if node.children.is_empty() {
                false // Leaf nodes can't be validated through children
            } else {
                node.children
                    .iter()
                    .all(|child| compute_node_validation(child, directly_validated, validation))
            };

            // A task is validated if it has a passing eval OR all children are validated
            let is_validated = directly_validated.contains(&node.id)
                || (!node.children.is_empty() && children_validated);

            validation.insert(node.id.clone(), is_validated);
            is_validated
        }

        for root in tasks {
            compute_node_validation(root, &directly_validated, &mut validation);
        }

        validation
    }

    // ========== BOOKMARK OPERATIONS ==========

    /// Save a bookmark
    pub fn save_bookmark(&self, name: &str, x: f64, y: f64, zoom: f64) -> BoardResult<Bookmark> {
        let db = self.open_db()?;
        let id = self.generate_slug(&db, "board_bookmarks", name)?;
        let now = self.now();

        db.execute(
            "INSERT INTO board_bookmarks (id, project_id, name, x, y, zoom, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![&id, self.project_id, name, x, y, zoom, &now],
        )?;

        self.get_bookmark(&id)
    }

    /// Get a bookmark by ID
    pub fn get_bookmark(&self, id: &str) -> BoardResult<Bookmark> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, name, x, y, zoom, created_at FROM board_bookmarks WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            Ok(Bookmark {
                id: row.get("id")?,
                name: row.get("name")?,
                x: row.get("x")?,
                y: row.get("y")?,
                zoom: row.get("zoom")?,
                created_at: row.get("created_at")?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => BoardError::BookmarkNotFound(id.to_string()),
            e => BoardError::Database(e),
        })
    }

    /// List all bookmarks
    pub fn list_bookmarks(&self) -> BoardResult<Vec<Bookmark>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, name, x, y, zoom, created_at FROM board_bookmarks WHERE project_id = ?1 ORDER BY created_at",
        )?;

        let bookmarks = stmt
            .query_map([self.project_id], |row| {
                Ok(Bookmark {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    x: row.get("x")?,
                    y: row.get("y")?,
                    zoom: row.get("zoom")?,
                    created_at: row.get("created_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(bookmarks)
    }

    /// Delete a bookmark
    pub fn delete_bookmark(&self, id: &str) -> BoardResult<()> {
        let db = self.open_db()?;
        let deleted = db.execute(
            "DELETE FROM board_bookmarks WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(BoardError::BookmarkNotFound(id.to_string()));
        }
        Ok(())
    }

    // ========== JSON EXPORT/IMPORT FOR GYP ==========

    /// Export board to board.json and establish baseline
    pub async fn export_for_agent(&mut self) -> BoardResult<PathBuf> {
        if self.should_use_remote() {
            return self.export_remote().await;
        }
        self.export_local()
    }

    fn export_local(&mut self) -> BoardResult<PathBuf> {
        let board_dir = self.ensure_board_dir()?;
        let task_tree = self.get_task_tree()?;
        let evals = self.get_evals()?;

        let board_json = BoardJson {
            version: 2,
            tasks: task_tree,
            evals,
        };

        let json = serde_json::to_string_pretty(&board_json)?;
        let path = self.board_json_path();
        std::fs::write(&path, json.as_bytes())?;

        // Store baseline hash
        self.baseline_hash = Some(self.hash_content(&json));
        self.last_sync_time = Some(SystemTime::now());

        info!("Exported board to {:?}", path);
        Ok(board_dir)
    }

    /// Import changes from board.json (baseline-diff sync)
    pub async fn import_from_agent(&mut self) -> BoardResult<SyncResult> {
        if self.should_use_remote() {
            return self.import_remote().await;
        }
        self.import_local()
    }

    fn import_local(&mut self) -> BoardResult<SyncResult> {
        let path = self.board_json_path();
        if !path.exists() {
            return Ok(SyncResult::default());
        }

        let content = std::fs::read_to_string(&path)?;
        let current_hash = self.hash_content(&content);

        // Check if file changed from baseline
        if let Some(ref baseline) = self.baseline_hash {
            if baseline == &current_hash {
                debug!("Board file unchanged from baseline, skipping import");
                return Ok(SyncResult::default());
            }
        }

        let board_json: BoardJson = serde_json::from_str(&content)?;
        let result = self.import_board_json(&board_json)?;

        // Update baseline
        self.baseline_hash = Some(current_hash);
        self.last_sync_time = Some(SystemTime::now());

        info!(
            "Imported board: tasks added={}, updated={}, deleted={}; evals added={}, updated={}, deleted={}",
            result.tasks_added.len(),
            result.tasks_updated.len(),
            result.tasks_deleted.len(),
            result.evals_added.len(),
            result.evals_updated.len(),
            result.evals_deleted.len()
        );

        Ok(result)
    }

    /// Import board JSON data
    fn import_board_json(&self, board_json: &BoardJson) -> BoardResult<SyncResult> {
        let db = self.open_db()?;
        let now = self.now();
        let mut result = SyncResult::default();

        // === Import Tasks ===
        let existing_task_ids: HashSet<String> = {
            let mut stmt = db.prepare("SELECT id FROM board_tasks WHERE project_id = ?1")?;
            let rows = stmt.query_map([self.project_id], |row| row.get(0))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };

        let mut seen_task_ids: HashSet<String> = HashSet::new();

        // Process tasks recursively
        fn process_task(
            db: &Connection,
            project_id: i64,
            task: &TaskTree,
            parent_id: Option<&str>,
            position: i32,
            now: &str,
            existing: &HashSet<String>,
            seen: &mut HashSet<String>,
            result: &mut SyncResult,
        ) -> rusqlite::Result<()> {
            seen.insert(task.id.clone());

            if existing.contains(&task.id) {
                // Update existing task
                db.execute(
                    "UPDATE board_tasks SET parent_id = ?1, position = ?2, name = ?3, status = ?4,
                     content = ?5, x = ?6, y = ?7, updated_at = ?8 WHERE id = ?9 AND project_id = ?10",
                    params![
                        parent_id,
                        position,
                        &task.name,
                        task.status.as_str(),
                        &task.content,
                        task.x,
                        task.y,
                        now,
                        &task.id,
                        project_id
                    ],
                )?;
                result.tasks_updated.push(task.id.clone());
            } else {
                // Insert new task
                db.execute(
                    "INSERT INTO board_tasks (id, project_id, parent_id, position, name, status,
                     content, x, y, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        &task.id,
                        project_id,
                        parent_id,
                        position,
                        &task.name,
                        task.status.as_str(),
                        &task.content,
                        task.x,
                        task.y,
                        now,
                        now
                    ],
                )?;
                result.tasks_added.push(task.id.clone());
            }

            // Process children
            for (i, child) in task.children.iter().enumerate() {
                process_task(
                    db,
                    project_id,
                    child,
                    Some(&task.id),
                    i as i32,
                    now,
                    existing,
                    seen,
                    result,
                )?;
            }

            Ok(())
        }

        // Process root tasks
        for (i, task) in board_json.tasks.iter().enumerate() {
            process_task(
                &db,
                self.project_id,
                task,
                None,
                i as i32,
                &now,
                &existing_task_ids,
                &mut seen_task_ids,
                &mut result,
            )?;
        }

        // Delete tasks not in the file
        for id in &existing_task_ids {
            if !seen_task_ids.contains(id) {
                db.execute(
                    "DELETE FROM board_tasks WHERE id = ?1 AND project_id = ?2",
                    params![id, self.project_id],
                )?;
                result.tasks_deleted.push(id.clone());
            }
        }

        // === Import Evals ===
        let existing_eval_ids: HashSet<String> = {
            let mut stmt = db.prepare("SELECT id FROM board_evals WHERE project_id = ?1")?;
            let rows = stmt.query_map([self.project_id], |row| row.get(0))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };

        let mut seen_eval_ids: HashSet<String> = HashSet::new();

        for eval in &board_json.evals {
            seen_eval_ids.insert(eval.id.clone());
            let validates_json =
                serde_json::to_string(&eval.validates).unwrap_or_else(|_| "[]".to_string());

            if existing_eval_ids.contains(&eval.id) {
                // Update existing eval
                db.execute(
                    "UPDATE board_evals SET name = ?1, status = ?2, content = ?3, validates = ?4,
                     x = ?5, y = ?6, updated_at = ?7 WHERE id = ?8 AND project_id = ?9",
                    params![
                        &eval.name,
                        eval.status.as_str(),
                        &eval.content,
                        &validates_json,
                        eval.x,
                        eval.y,
                        &now,
                        &eval.id,
                        self.project_id
                    ],
                )?;
                result.evals_updated.push(eval.id.clone());
            } else {
                // Insert new eval
                db.execute(
                    "INSERT INTO board_evals (id, project_id, name, status, content, validates, x, y, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        &eval.id,
                        self.project_id,
                        &eval.name,
                        eval.status.as_str(),
                        &eval.content,
                        &validates_json,
                        eval.x,
                        eval.y,
                        &now,
                        &now
                    ],
                )?;
                result.evals_added.push(eval.id.clone());
            }
        }

        // Delete evals not in the file
        for id in &existing_eval_ids {
            if !seen_eval_ids.contains(id) {
                db.execute(
                    "DELETE FROM board_evals WHERE id = ?1 AND project_id = ?2",
                    params![id, self.project_id],
                )?;
                result.evals_deleted.push(id.clone());
            }
        }

        result.changes = result.tasks_added.len()
            + result.tasks_updated.len()
            + result.tasks_deleted.len()
            + result.evals_added.len()
            + result.evals_updated.len()
            + result.evals_deleted.len();

        Ok(result)
    }

    /// Sync file changes to database (detect changes and import)
    pub async fn sync_file_changes(&mut self) -> BoardResult<SyncResult> {
        if self.should_use_remote() {
            return self.import_remote().await;
        }

        let path = self.board_json_path();
        if !path.exists() {
            return Ok(SyncResult::default());
        }

        // Check mtime
        let mtime = std::fs::metadata(&path)?.modified()?;
        if let Some(last) = self.last_sync_time {
            if mtime <= last {
                return Ok(SyncResult::default());
            }
        }

        self.import_local()
    }

    /// Get the project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    // ========== REMOTE MODE ==========

    async fn export_remote(&self) -> BoardResult<PathBuf> {
        let client = self.remote_client()?;
        let path: String = client
            .post(
                &format!("/api/board/{}/export", self.project_id),
                &serde_json::json!({}),
            )
            .await?;
        Ok(PathBuf::from(path))
    }

    async fn import_remote(&self) -> BoardResult<SyncResult> {
        let client = self.remote_client()?;
        let result: SyncResult = client
            .post(
                &format!("/api/board/{}/import", self.project_id),
                &serde_json::json!({}),
            )
            .await?;
        Ok(result)
    }

    // ========== SYNC VARIANTS (for blocking contexts) ==========

    pub fn export_local_sync(&mut self) -> BoardResult<PathBuf> {
        self.export_local()
    }

    pub fn import_local_sync(&mut self) -> BoardResult<SyncResult> {
        self.import_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_test_db() -> (PathBuf, i64) {
        let dir = tempdir().unwrap();
        let db_path = dir.into_path().join("hirsel.db");

        // Create projects table
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO projects (id, name) VALUES (1, 'Test Project');",
        )
        .unwrap();

        // Create board tables
        db.execute_batch(SCHEMA).unwrap();

        (db_path, 1)
    }

    #[test]
    fn test_task_crud() {
        let (_db_path, project_id) = setup_test_db();
        let service = BoardService::new(project_id);

        // Create root task
        let root = service
            .create_task(&CreateTaskRequest {
                parent_id: None,
                name: "Build API".to_string(),
                content: "Implement REST API".to_string(),
            })
            .unwrap();

        assert_eq!(root.name, "Build API");
        assert_eq!(root.id, "build-api"); // Slug ID
        assert!(root.parent_id.is_none());

        // Create child
        let child = service
            .create_task(&CreateTaskRequest {
                parent_id: Some(root.id.clone()),
                name: "User Endpoints".to_string(),
                content: "CRUD for users".to_string(),
            })
            .unwrap();

        assert_eq!(child.parent_id, Some(root.id.clone()));
        assert_eq!(child.id, "user-endpoints");

        // Update
        let updated = service
            .update_task(
                &child.id,
                &UpdateTaskRequest {
                    status: Some(TaskStatus::Doing),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(updated.status, TaskStatus::Doing);

        // Get tree
        let tree = service.get_task_tree().unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 1);

        // Delete
        service.delete_task(&root.id).unwrap();
        let tree = service.get_task_tree().unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn test_eval_crud() {
        let (_db_path, project_id) = setup_test_db();
        let service = BoardService::new(project_id);

        // Create a task first
        let task = service
            .create_task(&CreateTaskRequest {
                parent_id: None,
                name: "Build API".to_string(),
                content: "".to_string(),
            })
            .unwrap();

        // Create eval
        let eval = service
            .create_eval(&CreateEvalRequest {
                name: "API Test".to_string(),
                content: "Test endpoints".to_string(),
                validates: vec![task.id.clone()],
            })
            .unwrap();

        assert_eq!(eval.name, "API Test");
        assert_eq!(eval.id, "api-test");
        assert_eq!(eval.validates, vec![task.id.clone()]);
        assert_eq!(eval.status, EvalStatus::Blocked);

        // Update eval status
        let updated = service
            .update_eval(
                &eval.id,
                &UpdateEvalRequest {
                    status: Some(EvalStatus::Passed),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(updated.status, EvalStatus::Passed);

        // Check validation
        let tree = service.get_task_tree().unwrap();
        assert!(tree[0].validated.unwrap_or(false));

        // Delete eval
        service.delete_eval(&eval.id).unwrap();
        assert!(service.get_evals().unwrap().is_empty());
    }

    #[test]
    fn test_validation_computation() {
        let (_db_path, project_id) = setup_test_db();
        let service = BoardService::new(project_id);

        // Create parent with two children
        let parent = service
            .create_task(&CreateTaskRequest {
                parent_id: None,
                name: "Parent".to_string(),
                content: "".to_string(),
            })
            .unwrap();

        let child1 = service
            .create_task(&CreateTaskRequest {
                parent_id: Some(parent.id.clone()),
                name: "Child 1".to_string(),
                content: "".to_string(),
            })
            .unwrap();

        let child2 = service
            .create_task(&CreateTaskRequest {
                parent_id: Some(parent.id.clone()),
                name: "Child 2".to_string(),
                content: "".to_string(),
            })
            .unwrap();

        // Create passing evals for both children
        service
            .create_eval(&CreateEvalRequest {
                name: "Eval 1".to_string(),
                content: "".to_string(),
                validates: vec![child1.id.clone()],
            })
            .unwrap();

        service
            .update_eval(
                "eval-1",
                &UpdateEvalRequest {
                    status: Some(EvalStatus::Passed),
                    ..Default::default()
                },
            )
            .unwrap();

        // Only one child validated - parent should NOT be validated
        let tree = service.get_task_tree().unwrap();
        assert!(tree[0].children[0].validated.unwrap_or(false)); // child1 validated
        assert!(!tree[0].children[1].validated.unwrap_or(true)); // child2 not validated
        assert!(!tree[0].validated.unwrap_or(true)); // parent not validated

        // Now validate child2
        let eval2 = service
            .create_eval(&CreateEvalRequest {
                name: "Eval 2".to_string(),
                content: "".to_string(),
                validates: vec![child2.id.clone()],
            })
            .unwrap();

        service
            .update_eval(
                &eval2.id,
                &UpdateEvalRequest {
                    status: Some(EvalStatus::Passed),
                    ..Default::default()
                },
            )
            .unwrap();

        // Now parent should be validated (all children validated)
        let tree = service.get_task_tree().unwrap();
        assert!(tree[0].validated.unwrap_or(false)); // parent validated
    }

    #[test]
    fn test_bookmarks() {
        let (_db_path, project_id) = setup_test_db();
        let service = BoardService::new(project_id);

        let bm = service
            .save_bookmark("Overview", 100.0, 200.0, 1.5)
            .unwrap();
        assert_eq!(bm.name, "Overview");

        let bookmarks = service.list_bookmarks().unwrap();
        assert_eq!(bookmarks.len(), 1);

        service.delete_bookmark(&bm.id).unwrap();
        assert!(service.list_bookmarks().unwrap().is_empty());
    }
}
