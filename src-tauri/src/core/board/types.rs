//! SpecFlow Board Types
//!
//! The board has two types of entities:
//! - **Tasks**: Nested tree of work items (post-it style)
//! - **Evals**: Flat list of verifications that validate tasks
//!
//! ## Validation Rules
//! A task is "validated" if:
//! 1. It has at least one eval with status=passed that references it, OR
//! 2. All of its children are validated
//!
//! Validation propagates up the tree automatically.

use serde::{Deserialize, Serialize};

// =============================================================================
// Task Types
// =============================================================================

/// Task status for board tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    Doing,
    Done,
    Blocked,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Done => "done",
            Self::Blocked => "blocked",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "doing" => Self::Doing,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            _ => Self::Todo,
        }
    }
}

/// A task in the board (flat, for DB storage)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String, // Slug ID (e.g., "build-api")
    pub parent_id: Option<String>,
    pub position: i32,
    pub name: String,
    pub status: TaskStatus,
    pub content: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Task tree (nested, for JSON/frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTree {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub children: Vec<TaskTree>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validated: Option<bool>, // Computed field
}

impl From<Task> for TaskTree {
    fn from(task: Task) -> Self {
        Self {
            id: task.id,
            name: task.name,
            status: task.status,
            content: task.content,
            children: vec![],
            x: task.x,
            y: task.y,
            validated: None,
        }
    }
}

// =============================================================================
// Eval Types
// =============================================================================

/// Eval status values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvalStatus {
    #[default]
    Blocked, // Cannot run yet (dependencies not ready)
    Queued,     // Ready to run
    InProgress, // Currently running
    Passed,     // Verification succeeded
    Failed,     // Verification failed
}

impl EvalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Queued => "queued",
            Self::InProgress => "in_progress",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "in_progress" => Self::InProgress,
            "passed" => Self::Passed,
            "failed" => Self::Failed,
            _ => Self::Blocked,
        }
    }
}

/// An eval (verification) in the board
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eval {
    pub id: String, // Slug ID (e.g., "api-test")
    pub name: String,
    #[serde(default)]
    pub status: EvalStatus,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub validates: Vec<String>, // Task IDs this eval validates
    pub x: Option<f64>,
    pub y: Option<f64>,
    #[serde(skip)] // Internal metadata, not for Gyp
    pub created_at: String,
    #[serde(skip)]
    pub updated_at: String,
}

// =============================================================================
// Request Types
// =============================================================================

/// Request to create a new task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub content: String,
}

/// Request to update a task
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}

/// Request to create a new eval
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEvalRequest {
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub validates: Vec<String>,
}

/// Request to update an eval
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEvalRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<EvalStatus>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub validates: Option<Vec<String>>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}

// =============================================================================
// JSON Export/Import Types
// =============================================================================

/// Board JSON format for agents (version 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardJson {
    pub version: u32,
    pub tasks: Vec<TaskTree>,
    pub evals: Vec<Eval>,
}

impl Default for BoardJson {
    fn default() -> Self {
        Self {
            version: 2,
            tasks: vec![],
            evals: vec![],
        }
    }
}

/// Result of a sync operation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncResult {
    /// Number of changes applied
    pub changes: usize,
    /// Tasks that were added
    pub tasks_added: Vec<String>,
    /// Tasks that were updated
    pub tasks_updated: Vec<String>,
    /// Tasks that were deleted
    pub tasks_deleted: Vec<String>,
    /// Evals that were added
    pub evals_added: Vec<String>,
    /// Evals that were updated
    pub evals_updated: Vec<String>,
    /// Evals that were deleted
    pub evals_deleted: Vec<String>,
}

/// A saved viewport position (bookmark)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
    pub created_at: String,
}

// =============================================================================
// Dispatch Types
// =============================================================================

/// A record of a run dispatched from a task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub id: i64,
    pub project_id: i64,
    pub task_id: String,
    pub run_name: String,
    pub dispatched_at: String,
}

/// Preview of what will be dispatched from a task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPreview {
    pub task_ids: Vec<String>,
    pub eval_ids: Vec<String>,
    pub task_count: usize,
    pub eval_count: usize,
}

/// Board snapshot taken at dispatch time
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSnapshot {
    pub tasks: Vec<TaskTree>,
    pub evals: Vec<Eval>,
    pub dispatched_at: String,
}

// =============================================================================
// Per-Task File Storage Types
// =============================================================================

/// A task file containing a top-level task tree and related evals
///
/// This is the minimal format Gyp sees - just the task and evals, nothing else.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFile {
    /// The top-level task and its subtree
    pub task: TaskTree,
    /// Evals that validate any task in this subtree
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evals: Vec<Eval>,
}

/// Scope for board export/context operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportScope {
    /// Whole board - all tasks exported as separate files
    WholeBoard,
    /// Focused on a specific task tree
    FocusedTask { task_id: String, task_name: String },
}

impl Default for ExportScope {
    fn default() -> Self {
        Self::WholeBoard
    }
}
