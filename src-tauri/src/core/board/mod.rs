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
//! - **Evals**: Flat list that validate tasks
//!
//! ## Agent Access
//!
//! Board structure is exposed via MCP tools (board_view, board_task, etc.)
//! Content files live at: `~/.hirsel/projects/{project_id}/board/tasks/{id}.md`
//!
//! The agent uses MCP tools for structure, direct file edits for content.

pub mod mcp;
pub mod storage;
mod types;

pub use storage::{create_board_storage, BoardStorage, LocalBoardStorage, RemoteBoardStorage};
pub use types::{
    BoardJson, BoardSnapshot, Bookmark, CreateEvalRequest, CreateTaskRequest, DispatchPreview,
    Eval, EvalStatus, ExportScope, SyncResult, Task, TaskFile, TaskRun, TaskStatus, TaskTree,
    UpdateEvalRequest, UpdateTaskRequest,
};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use tokio::sync::OnceCell;

use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use tracing::{debug, info};

use crate::core::config::{hirsel_dir, OrchestratorMode, OrchestratorProfile};
use crate::core::db::{global_pool, utc_now};
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

-- Task-Run junction table (tracks which runs were dispatched from which tasks)
CREATE TABLE IF NOT EXISTS task_runs (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    task_id TEXT NOT NULL,
    run_name TEXT NOT NULL,
    dispatched_at TEXT NOT NULL,
    UNIQUE(project_id, task_id, run_name)
);

-- File baselines for change detection (hash of last exported content)
CREATE TABLE IF NOT EXISTS board_file_baselines (
    project_id INTEGER NOT NULL,
    task_slug TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (project_id, task_slug)
);

CREATE INDEX IF NOT EXISTS idx_board_tasks_project ON board_tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_board_tasks_parent ON board_tasks(parent_id);
CREATE INDEX IF NOT EXISTS idx_board_evals_project ON board_evals(project_id);
CREATE INDEX IF NOT EXISTS idx_board_bookmarks_project ON board_bookmarks(project_id);
CREATE INDEX IF NOT EXISTS idx_task_runs_project ON task_runs(project_id);
CREATE INDEX IF NOT EXISTS idx_task_runs_task ON task_runs(task_id);
CREATE INDEX IF NOT EXISTS idx_task_runs_run ON task_runs(run_name);
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

/// Error type for board operations
#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
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
}

impl BoardService {
    /// Create a new board service for local mode
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            profile: None,
            last_sync_time: None,
        }
    }

    /// Create a board service with a profile (for remote mode)
    pub fn with_profile(project_id: i64, profile: OrchestratorProfile) -> Self {
        Self {
            project_id,
            profile: Some(profile),
            last_sync_time: None,
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

    /// Get pool and ensure schema
    async fn pool(&self) -> BoardResult<&'static SqlitePool> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(pool)
    }

    /// Compute a simple hash of content for baseline comparison
    fn hash_content(&self, content: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Generate a unique slug ID from a name
    async fn generate_slug(&self, table: &str, name: &str) -> BoardResult<String> {
        let pool = self.pool().await?;
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
            let sql = format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ? AND project_id = ?)",
                table
            );
            let exists: bool = sqlx::query_scalar(&sql)
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

    // ========== TASK CRUD OPERATIONS ==========

    /// Get all tasks for a project as flat list
    pub async fn get_tasks(&self) -> BoardResult<Vec<Task>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, parent_id, position, name, status, content, x, y, created_at, updated_at
             FROM board_tasks
             WHERE project_id = ?
             ORDER BY parent_id NULLS FIRST, position",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let tasks = rows
            .into_iter()
            .map(|row| Self::row_to_task(&row))
            .collect();

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
    pub async fn get_task_tree(&self) -> BoardResult<Vec<TaskTree>> {
        let tasks = self.get_tasks().await?;
        let evals = self.get_evals().await?;
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
    pub async fn create_task(&self, req: &CreateTaskRequest) -> BoardResult<Task> {
        let pool = self.pool().await?;
        let id = self.generate_slug("board_tasks", &req.name).await?;
        let now = utc_now();

        // Get position (append to end of siblings)
        let position: i32 = match &req.parent_id {
            Some(parent_id) => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM board_tasks WHERE parent_id = ? AND project_id = ?",
                )
                .bind(parent_id)
                .bind(self.project_id)
                .fetch_optional(pool)
                .await?
                .flatten();
                max.unwrap_or(-1) + 1
            }
            None => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM board_tasks WHERE parent_id IS NULL AND project_id = ?",
                )
                .bind(self.project_id)
                .fetch_optional(pool)
                .await?
                .flatten();
                max.unwrap_or(-1) + 1
            }
        };

        sqlx::query(
            "INSERT INTO board_tasks (id, project_id, parent_id, position, name, content, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(self.project_id)
        .bind(&req.parent_id)
        .bind(position)
        .bind(&req.name)
        .bind(&req.content)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        self.get_task(&id).await
    }

    /// Get a task by ID
    pub async fn get_task(&self, id: &str) -> BoardResult<Task> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, parent_id, position, name, status, content, x, y, created_at, updated_at
             FROM board_tasks
             WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| BoardError::TaskNotFound(id.to_string()))?;

        Ok(Self::row_to_task(&row))
    }

    /// Update a task
    pub async fn update_task(&self, id: &str, req: &UpdateTaskRequest) -> BoardResult<Task> {
        let pool = self.pool().await?;

        // Verify exists
        let _ = self.get_task(id).await?;

        let now = utc_now();
        let mut updates = vec!["updated_at = ?".to_string()];
        let mut bind_idx = 2usize;

        if req.name.is_some() {
            updates.push(format!("name = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.status.is_some() {
            updates.push(format!("status = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.content.is_some() {
            updates.push(format!("content = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.x.is_some() {
            updates.push(format!("x = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.y.is_some() {
            updates.push(format!("y = ?{}", bind_idx));
            bind_idx += 1;
        }

        let sql = format!(
            "UPDATE board_tasks SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            bind_idx,
            bind_idx + 1
        );

        let mut query = sqlx::query(&sql).bind(&now);

        if let Some(ref name) = req.name {
            query = query.bind(name);
        }
        if let Some(status) = req.status {
            query = query.bind(status.as_str());
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

        self.get_task(id).await
    }

    /// Delete a task (and all descendants via CASCADE)
    pub async fn delete_task(&self, id: &str) -> BoardResult<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM board_tasks WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(BoardError::TaskNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Move a task to a new parent and/or position
    pub async fn move_task(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_position: i32,
    ) -> BoardResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE board_tasks SET parent_id = ?, position = ?, updated_at = ? WHERE id = ? AND project_id = ?",
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

    fn row_to_task(row: &SqliteRow) -> Task {
        Task {
            id: row.get("id"),
            parent_id: row.get("parent_id"),
            position: row.get("position"),
            name: row.get("name"),
            status: TaskStatus::from_str(&row.get::<String, _>("status")),
            content: row.get("content"),
            x: row.get("x"),
            y: row.get("y"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }

    // ========== EVAL CRUD OPERATIONS ==========

    /// Get all evals for a project
    pub async fn get_evals(&self) -> BoardResult<Vec<Eval>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, name, status, content, validates, x, y, created_at, updated_at
             FROM board_evals
             WHERE project_id = ?
             ORDER BY created_at",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let evals = rows
            .into_iter()
            .map(|row| Self::row_to_eval(&row))
            .collect();

        Ok(evals)
    }

    /// Create a new eval
    pub async fn create_eval(&self, req: &CreateEvalRequest) -> BoardResult<Eval> {
        let pool = self.pool().await?;
        let id = self.generate_slug("board_evals", &req.name).await?;
        let now = utc_now();
        let validates_json = serde_json::to_string(&req.validates)?;

        sqlx::query(
            "INSERT INTO board_evals (id, project_id, name, content, validates, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(self.project_id)
        .bind(&req.name)
        .bind(&req.content)
        .bind(&validates_json)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        self.get_eval(&id).await
    }

    /// Get an eval by ID
    pub async fn get_eval(&self, id: &str) -> BoardResult<Eval> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, name, status, content, validates, x, y, created_at, updated_at
             FROM board_evals
             WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| BoardError::EvalNotFound(id.to_string()))?;

        Ok(Self::row_to_eval(&row))
    }

    /// Update an eval
    pub async fn update_eval(&self, id: &str, req: &UpdateEvalRequest) -> BoardResult<Eval> {
        let pool = self.pool().await?;

        // Verify exists
        let _ = self.get_eval(id).await?;

        let now = utc_now();
        let mut updates = vec!["updated_at = ?".to_string()];
        let mut bind_idx = 2usize;

        if req.name.is_some() {
            updates.push(format!("name = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.status.is_some() {
            updates.push(format!("status = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.content.is_some() {
            updates.push(format!("content = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.validates.is_some() {
            updates.push(format!("validates = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.x.is_some() {
            updates.push(format!("x = ?{}", bind_idx));
            bind_idx += 1;
        }
        if req.y.is_some() {
            updates.push(format!("y = ?{}", bind_idx));
            bind_idx += 1;
        }

        let sql = format!(
            "UPDATE board_evals SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            bind_idx,
            bind_idx + 1
        );

        let mut query = sqlx::query(&sql).bind(&now);

        if let Some(ref name) = req.name {
            query = query.bind(name);
        }
        if let Some(status) = req.status {
            query = query.bind(status.as_str());
        }
        if let Some(ref content) = req.content {
            query = query.bind(content);
        }
        if let Some(ref validates) = req.validates {
            let validates_json =
                serde_json::to_string(validates).unwrap_or_else(|_| "[]".to_string());
            query = query.bind(validates_json);
        }
        if let Some(x) = req.x {
            query = query.bind(x);
        }
        if let Some(y) = req.y {
            query = query.bind(y);
        }

        query = query.bind(id).bind(self.project_id);
        query.execute(pool).await?;

        self.get_eval(id).await
    }

    /// Delete an eval
    pub async fn delete_eval(&self, id: &str) -> BoardResult<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM board_evals WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(BoardError::EvalNotFound(id.to_string()));
        }
        Ok(())
    }

    fn row_to_eval(row: &SqliteRow) -> Eval {
        let validates_json: String = row.get("validates");
        let validates: Vec<String> = serde_json::from_str(&validates_json).unwrap_or_default();

        Eval {
            id: row.get("id"),
            name: row.get("name"),
            status: EvalStatus::from_str(&row.get::<String, _>("status")),
            content: row.get("content"),
            validates,
            x: row.get("x"),
            y: row.get("y"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
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
    pub async fn save_bookmark(
        &self,
        name: &str,
        x: f64,
        y: f64,
        zoom: f64,
    ) -> BoardResult<Bookmark> {
        let pool = self.pool().await?;
        let id = self.generate_slug("board_bookmarks", name).await?;
        let now = utc_now();

        sqlx::query(
            "INSERT INTO board_bookmarks (id, project_id, name, x, y, zoom, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(self.project_id)
        .bind(name)
        .bind(x)
        .bind(y)
        .bind(zoom)
        .bind(&now)
        .execute(pool)
        .await?;

        self.get_bookmark(&id).await
    }

    /// Get a bookmark by ID
    pub async fn get_bookmark(&self, id: &str) -> BoardResult<Bookmark> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, name, x, y, zoom, created_at FROM board_bookmarks WHERE id = ? AND project_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| BoardError::BookmarkNotFound(id.to_string()))?;

        Ok(Bookmark {
            id: row.get("id"),
            name: row.get("name"),
            x: row.get("x"),
            y: row.get("y"),
            zoom: row.get("zoom"),
            created_at: row.get("created_at"),
        })
    }

    /// List all bookmarks
    pub async fn list_bookmarks(&self) -> BoardResult<Vec<Bookmark>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, name, x, y, zoom, created_at FROM board_bookmarks WHERE project_id = ? ORDER BY created_at",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let bookmarks = rows
            .into_iter()
            .map(|row| Bookmark {
                id: row.get("id"),
                name: row.get("name"),
                x: row.get("x"),
                y: row.get("y"),
                zoom: row.get("zoom"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(bookmarks)
    }

    /// Delete a bookmark
    pub async fn delete_bookmark(&self, id: &str) -> BoardResult<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM board_bookmarks WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(self.project_id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(BoardError::BookmarkNotFound(id.to_string()));
        }
        Ok(())
    }

    // ========== BASELINE TRACKING (DB) ==========

    /// Get the baseline hash for a task file from DB
    async fn get_baseline_hash(&self, slug: &str) -> BoardResult<Option<String>> {
        let pool = self.pool().await?;
        let hash: Option<String> = sqlx::query_scalar(
            "SELECT content_hash FROM board_file_baselines WHERE project_id = ? AND task_slug = ?",
        )
        .bind(self.project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await?;
        Ok(hash)
    }

    /// Set the baseline hash for a task file in DB
    async fn set_baseline_hash(&self, slug: &str, hash: &str) -> BoardResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();
        sqlx::query(
            "INSERT INTO board_file_baselines (project_id, task_slug, content_hash, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(project_id, task_slug) DO UPDATE SET content_hash = excluded.content_hash, updated_at = excluded.updated_at",
        )
        .bind(self.project_id)
        .bind(slug)
        .bind(hash)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a baseline entry
    async fn delete_baseline(&self, slug: &str) -> BoardResult<()> {
        let pool = self.pool().await?;
        sqlx::query("DELETE FROM board_file_baselines WHERE project_id = ? AND task_slug = ?")
            .bind(self.project_id)
            .bind(slug)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Get all baseline slugs for this project
    async fn get_all_baseline_slugs(&self) -> BoardResult<HashSet<String>> {
        let pool = self.pool().await?;
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT task_slug FROM board_file_baselines WHERE project_id = ?",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    // ========== JSON EXPORT/IMPORT FOR GYP ==========

    /// Export board to per-task JSON files and establish baseline
    ///
    /// Each top-level task gets its own file: `{task-slug}.json`
    /// Evals are included in each file if they validate any task in that subtree.
    pub async fn export_for_agent(&mut self, scope: &ExportScope) -> BoardResult<PathBuf> {
        if self.should_use_remote() {
            return self.export_remote(scope).await;
        }
        self.export_local(scope).await
    }

    async fn export_local(&mut self, scope: &ExportScope) -> BoardResult<PathBuf> {
        let board_dir = self.ensure_board_dir()?;
        let task_tree = self.get_task_tree().await?;
        let evals = self.get_evals().await?;

        // Determine which tasks to export based on scope
        let tasks_to_export: Vec<&TaskTree> = match scope {
            ExportScope::WholeBoard => task_tree.iter().collect(),
            ExportScope::FocusedTask { task_id, .. } => {
                task_tree.iter().filter(|t| &t.id == task_id).collect()
            }
        };

        let mut exported_slugs: HashSet<String> = HashSet::new();

        for task in &tasks_to_export {
            // Collect all task IDs in this subtree
            let subtree_ids = Self::collect_subtree_ids(task);

            // Find evals that validate any task in this subtree
            let related_evals: Vec<Eval> = evals
                .iter()
                .filter(|eval| eval.validates.iter().any(|t| subtree_ids.contains(t)))
                .cloned()
                .collect();

            // Build task file (minimal - just task + evals)
            let task_file = TaskFile {
                task: (*task).clone(),
                evals: related_evals,
            };

            // Compute hash and write file
            let json = serde_json::to_string_pretty(&task_file)?;
            let content_hash = self.hash_content(&json);
            let path = board_dir.join(format!("{}.json", task.id));
            std::fs::write(&path, json.as_bytes())?;

            // Save baseline to DB
            self.set_baseline_hash(&task.id, &content_hash).await?;
            exported_slugs.insert(task.id.clone());
            debug!("Exported task file: {:?}", path);
        }

        // For whole board export, delete stale files and baselines
        if matches!(scope, ExportScope::WholeBoard) {
            let baseline_slugs = self.get_all_baseline_slugs().await?;
            for slug in baseline_slugs {
                if !exported_slugs.contains(&slug) {
                    let path = board_dir.join(format!("{}.json", slug));
                    if path.exists() {
                        std::fs::remove_file(&path)?;
                        debug!("Deleted stale task file: {:?}", path);
                    }
                    self.delete_baseline(&slug).await?;
                }
            }
        }

        self.last_sync_time = Some(SystemTime::now());
        info!("Exported board to {:?}", board_dir);
        Ok(board_dir)
    }

    /// Collect all task IDs in a subtree
    fn collect_subtree_ids(task: &TaskTree) -> HashSet<String> {
        let mut ids = HashSet::new();
        ids.insert(task.id.clone());
        for child in &task.children {
            ids.extend(Self::collect_subtree_ids(child));
        }
        ids
    }

    /// Import changes from per-task JSON files (baseline-diff sync)
    pub async fn import_from_agent(&mut self) -> BoardResult<SyncResult> {
        if self.should_use_remote() {
            return self.import_remote().await;
        }
        self.import_local().await
    }

    async fn import_local(&mut self) -> BoardResult<SyncResult> {
        let board_dir = self.board_dir();
        if !board_dir.exists() {
            return Ok(SyncResult::default());
        }

        let mut result = SyncResult::default();
        let mut seen_slugs: HashSet<String> = HashSet::new();
        let mut all_evals: HashMap<String, Eval> = HashMap::new();

        // Read all task files
        for entry in std::fs::read_dir(&board_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Skip non-json files
            if path.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            let slug = match path.file_stem() {
                Some(s) => s.to_string_lossy().to_string(),
                None => continue,
            };

            seen_slugs.insert(slug.clone());

            // Read and parse file
            let content = std::fs::read_to_string(&path)?;
            let task_file: TaskFile = serde_json::from_str(&content)?;

            // Compute current hash
            let current_hash = self.hash_content(&content);

            // Check if changed from baseline (in DB)
            if let Some(baseline_hash) = self.get_baseline_hash(&slug).await? {
                if baseline_hash == current_hash {
                    debug!("Task file {} unchanged from baseline, skipping", slug);
                    continue;
                }
            }

            debug!("Importing changed task file: {}", slug);

            // Import task tree
            self.import_task_tree(&task_file.task, None, 0, &mut result)
                .await?;

            // Collect evals (will be deduplicated by ID)
            for eval in task_file.evals {
                all_evals.insert(eval.id.clone(), eval);
            }

            // Update baseline in DB
            self.set_baseline_hash(&slug, &current_hash).await?;
        }

        // Import all collected evals
        self.import_evals(&all_evals.into_values().collect::<Vec<_>>(), &mut result)
            .await?;

        // Handle deletions: baselines in DB but file not in directory
        let baseline_slugs = self.get_all_baseline_slugs().await?;
        let stale_slugs: Vec<String> = baseline_slugs
            .into_iter()
            .filter(|s| !seen_slugs.contains(s))
            .collect();

        for slug in stale_slugs {
            // Delete this task tree from DB
            self.delete_task_cascade(&slug).await?;
            result.tasks_deleted.push(slug.clone());
            self.delete_baseline(&slug).await?;
        }

        result.changes = result.tasks_added.len()
            + result.tasks_updated.len()
            + result.tasks_deleted.len()
            + result.evals_added.len()
            + result.evals_updated.len()
            + result.evals_deleted.len();

        self.last_sync_time = Some(SystemTime::now());

        if result.changes > 0 {
            info!(
                "Imported board: tasks added={}, updated={}, deleted={}; evals added={}, updated={}, deleted={}",
                result.tasks_added.len(),
                result.tasks_updated.len(),
                result.tasks_deleted.len(),
                result.evals_added.len(),
                result.evals_updated.len(),
                result.evals_deleted.len()
            );
        }

        Ok(result)
    }

    /// Import a task tree recursively
    async fn import_task_tree(
        &self,
        task: &TaskTree,
        parent_id: Option<&str>,
        position: i32,
        result: &mut SyncResult,
    ) -> BoardResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Check if task exists
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM board_tasks WHERE id = ? AND project_id = ?)",
        )
        .bind(&task.id)
        .bind(self.project_id)
        .fetch_one(pool)
        .await?;

        if exists {
            // Update existing task
            sqlx::query(
                "UPDATE board_tasks SET parent_id = ?, position = ?, name = ?, status = ?,
                 content = ?, x = ?, y = ?, updated_at = ? WHERE id = ? AND project_id = ?",
            )
            .bind(parent_id)
            .bind(position)
            .bind(&task.name)
            .bind(task.status.as_str())
            .bind(&task.content)
            .bind(task.x)
            .bind(task.y)
            .bind(&now)
            .bind(&task.id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
            result.tasks_updated.push(task.id.clone());
        } else {
            // Insert new task
            sqlx::query(
                "INSERT INTO board_tasks (id, project_id, parent_id, position, name, status,
                 content, x, y, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&task.id)
            .bind(self.project_id)
            .bind(parent_id)
            .bind(position)
            .bind(&task.name)
            .bind(task.status.as_str())
            .bind(&task.content)
            .bind(task.x)
            .bind(task.y)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;
            result.tasks_added.push(task.id.clone());
        }

        // Process children
        for (i, child) in task.children.iter().enumerate() {
            Box::pin(self.import_task_tree(child, Some(&task.id), i as i32, result)).await?;
        }

        Ok(())
    }

    /// Import evals (upsert, deduped by ID)
    async fn import_evals(&self, evals: &[Eval], result: &mut SyncResult) -> BoardResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        for eval in evals {
            let validates_json =
                serde_json::to_string(&eval.validates).unwrap_or_else(|_| "[]".to_string());

            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM board_evals WHERE id = ? AND project_id = ?)",
            )
            .bind(&eval.id)
            .bind(self.project_id)
            .fetch_one(pool)
            .await?;

            if exists {
                sqlx::query(
                    "UPDATE board_evals SET name = ?, status = ?, content = ?, validates = ?,
                     x = ?, y = ?, updated_at = ? WHERE id = ? AND project_id = ?",
                )
                .bind(&eval.name)
                .bind(eval.status.as_str())
                .bind(&eval.content)
                .bind(&validates_json)
                .bind(eval.x)
                .bind(eval.y)
                .bind(&now)
                .bind(&eval.id)
                .bind(self.project_id)
                .execute(pool)
                .await?;
                result.evals_updated.push(eval.id.clone());
            } else {
                sqlx::query(
                    "INSERT INTO board_evals (id, project_id, name, status, content, validates, x, y, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&eval.id)
                .bind(self.project_id)
                .bind(&eval.name)
                .bind(eval.status.as_str())
                .bind(&eval.content)
                .bind(&validates_json)
                .bind(eval.x)
                .bind(eval.y)
                .bind(&now)
                .bind(&now)
                .execute(pool)
                .await?;
                result.evals_added.push(eval.id.clone());
            }
        }

        Ok(())
    }

    /// Delete a task and all its descendants (cascade delete)
    async fn delete_task_cascade(&self, task_id: &str) -> BoardResult<()> {
        let pool = self.pool().await?;
        // CASCADE will handle children
        sqlx::query("DELETE FROM board_tasks WHERE id = ? AND project_id = ?")
            .bind(task_id)
            .bind(self.project_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Sync file changes to database (detect changes and import)
    pub async fn sync_file_changes(&mut self) -> BoardResult<SyncResult> {
        if self.should_use_remote() {
            return self.import_remote().await;
        }

        let board_dir = self.board_dir();
        if !board_dir.exists() {
            return Ok(SyncResult::default());
        }

        // Check if any file was modified since last sync
        let mut any_changed = false;
        if let Some(last) = self.last_sync_time {
            for entry in std::fs::read_dir(&board_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if let Ok(mtime) = meta.modified() {
                            if mtime > last {
                                any_changed = true;
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            any_changed = true;
        }

        if !any_changed {
            return Ok(SyncResult::default());
        }

        self.import_local().await
    }

    /// Get the project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    // ========== REMOTE MODE ==========

    async fn export_remote(&self, scope: &ExportScope) -> BoardResult<PathBuf> {
        let client = self.remote_client()?;
        let path: String = client
            .post(
                &format!("/api/board/{}/export", self.project_id),
                &serde_json::json!({ "scope": scope }),
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

    pub async fn export_local_sync(&mut self, scope: &ExportScope) -> BoardResult<PathBuf> {
        self.export_local(scope).await
    }

    pub async fn import_local_sync(&mut self) -> BoardResult<SyncResult> {
        self.import_local().await
    }

    // ========== TASK-RUN TRACKING ==========

    /// Record that a run was dispatched from a task
    pub async fn record_task_run(&self, task_id: &str, run_name: &str) -> BoardResult<TaskRun> {
        let pool = self.pool().await?;
        let now = utc_now();

        let result = sqlx::query(
            "INSERT INTO task_runs (project_id, task_id, run_name, dispatched_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(self.project_id)
        .bind(task_id)
        .bind(run_name)
        .bind(&now)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(TaskRun {
            id,
            project_id: self.project_id,
            task_id: task_id.to_string(),
            run_name: run_name.to_string(),
            dispatched_at: now,
        })
    }

    /// Get all runs dispatched from a specific task
    pub async fn get_runs_for_task(&self, task_id: &str) -> BoardResult<Vec<TaskRun>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, task_id, run_name, dispatched_at
             FROM task_runs
             WHERE project_id = ? AND task_id = ?
             ORDER BY dispatched_at DESC",
        )
        .bind(self.project_id)
        .bind(task_id)
        .fetch_all(pool)
        .await?;

        let runs = rows
            .into_iter()
            .map(|row| TaskRun {
                id: row.get("id"),
                project_id: row.get("project_id"),
                task_id: row.get("task_id"),
                run_name: row.get("run_name"),
                dispatched_at: row.get("dispatched_at"),
            })
            .collect();

        Ok(runs)
    }

    /// Get all task_runs for this project (for showing satellites)
    pub async fn get_all_task_runs(&self) -> BoardResult<Vec<TaskRun>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, task_id, run_name, dispatched_at
             FROM task_runs
             WHERE project_id = ?
             ORDER BY dispatched_at DESC",
        )
        .bind(self.project_id)
        .fetch_all(pool)
        .await?;

        let runs = rows
            .into_iter()
            .map(|row| TaskRun {
                id: row.get("id"),
                project_id: row.get("project_id"),
                task_id: row.get("task_id"),
                run_name: row.get("run_name"),
                dispatched_at: row.get("dispatched_at"),
            })
            .collect();

        Ok(runs)
    }

    /// Delete a task_run record (e.g., when run is abandoned)
    pub async fn delete_task_run(&self, run_name: &str) -> BoardResult<()> {
        let pool = self.pool().await?;
        sqlx::query("DELETE FROM task_runs WHERE project_id = ? AND run_name = ?")
            .bind(self.project_id)
            .bind(run_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    // ========== DISPATCH HELPERS ==========

    /// Get all task IDs in a subtree (task + all descendants)
    pub async fn get_subtree_task_ids(&self, root_task_id: &str) -> BoardResult<Vec<String>> {
        let tasks = self.get_tasks().await?;
        let mut result = vec![root_task_id.to_string()];

        fn collect_descendants(parent_id: &str, tasks: &[Task], result: &mut Vec<String>) {
            for task in tasks {
                if task.parent_id.as_deref() == Some(parent_id) {
                    result.push(task.id.clone());
                    collect_descendants(&task.id, tasks, result);
                }
            }
        }

        collect_descendants(root_task_id, &tasks, &mut result);
        Ok(result)
    }

    /// Get evals that validate any of the given tasks
    pub async fn get_evals_for_tasks(&self, task_ids: &[String]) -> BoardResult<Vec<Eval>> {
        let evals = self.get_evals().await?;
        let task_id_set: HashSet<&String> = task_ids.iter().collect();

        let matching_evals = evals
            .into_iter()
            .filter(|eval| eval.validates.iter().any(|t| task_id_set.contains(t)))
            .collect();

        Ok(matching_evals)
    }

    /// Create a dispatch preview for a task subtree
    pub async fn preview_dispatch(&self, root_task_id: &str) -> BoardResult<DispatchPreview> {
        let task_ids = self.get_subtree_task_ids(root_task_id).await?;
        let evals = self.get_evals_for_tasks(&task_ids).await?;
        let eval_ids: Vec<String> = evals.iter().map(|e| e.id.clone()).collect();

        Ok(DispatchPreview {
            task_count: task_ids.len(),
            eval_count: eval_ids.len(),
            task_ids,
            eval_ids,
        })
    }

    /// Create a board snapshot for a dispatch
    pub async fn create_dispatch_snapshot(
        &self,
        task_ids: &[String],
    ) -> BoardResult<BoardSnapshot> {
        let tasks = self.get_tasks().await?;
        let task_id_set: HashSet<&String> = task_ids.iter().collect();

        // Filter tasks to only include those in the dispatch scope
        let filtered_tasks: Vec<Task> = tasks
            .into_iter()
            .filter(|t| task_id_set.contains(&t.id))
            .collect();

        // Build tree from filtered tasks
        let tree = self.build_task_tree(&filtered_tasks);

        // Get evals for these tasks
        let evals = self.get_evals_for_tasks(task_ids).await?;

        Ok(BoardSnapshot {
            tasks: tree,
            evals,
            dispatched_at: utc_now(),
        })
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
