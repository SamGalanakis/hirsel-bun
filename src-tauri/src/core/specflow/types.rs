//! SpecFlow types and data structures
//!
//! SpecFlow is a spatial canvas for managing project specs, tasks, and evals.
//! Each project has islands (feature containers) containing rows (trifecta: spec/task/eval).

use serde::{Deserialize, Serialize};

/// Task status for board rows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
    Blocked,
    Deleted,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Deleted => "deleted",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "doing" => Self::Doing,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "deleted" => Self::Deleted,
            _ => Self::Todo,
        }
    }
}

/// Spec status for board rows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecStatus {
    Draft,
    Approved,
}

impl SpecStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Approved => "approved",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "approved" => Self::Approved,
            _ => Self::Draft,
        }
    }
}

/// Eval status for board rows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowEvalStatus {
    Pending,
    Pass,
    Fail,
}

impl RowEvalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pass" => Self::Pass,
            "fail" => Self::Fail,
            _ => Self::Pending,
        }
    }
}

/// A row in the trifecta grid (spec | task | eval)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub island_id: String,
    pub position: i32,

    // Spec column (Intent)
    pub spec_content: Option<String>,
    pub spec_status: SpecStatus,

    // Task column (Reality)
    pub task_title: Option<String>,
    pub task_description: Option<String>,
    pub task_status: TaskStatus,
    pub task_worker: Option<String>,
    pub task_blocked_by: Vec<String>, // Row IDs this task depends on

    // Eval column (Proof)
    pub eval_criterion: Option<String>,
    pub eval_status: RowEvalStatus,
    pub eval_result: Option<String>,

    // Dispatch tracking
    pub dispatched: bool,
    pub run_name: Option<String>,

    pub created_at: String,
    pub updated_at: String,
}

/// An island (feature container) on the board
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Island {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub collapsed: bool,
    pub summary: Option<String>,
    pub rows: Vec<Row>,
    pub created_at: String,
    pub updated_at: String,
}

impl Island {
    /// Compute the island's status from its rows
    pub fn computed_status(&self) -> &'static str {
        let dispatched_rows: Vec<_> = self.rows.iter().filter(|r| r.dispatched).collect();
        if dispatched_rows.is_empty() {
            return "draft";
        }
        if dispatched_rows.len() < self.rows.len() {
            return "partial";
        }
        if dispatched_rows
            .iter()
            .all(|r| r.task_status == TaskStatus::Done)
        {
            return "done";
        }
        "dispatched"
    }
}

/// A dependency wire between islands (or rows)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wire {
    pub id: String,
    pub from_island_id: String,
    pub to_island_id: String,
    pub created_at: String,
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

/// Request to create an island
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIslandRequest {
    pub name: String,
    pub x: f64,
    pub y: f64,
    #[serde(default = "default_island_width")]
    pub width: f64,
}

fn default_island_width() -> f64 {
    400.0
}

/// Request to update an island
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIslandRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub collapsed: Option<bool>,
    #[serde(default)]
    pub summary: Option<String>,
}

/// Request to create a row
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRowRequest {
    pub island_id: String,
    #[serde(default)]
    pub position: Option<i32>, // If None, appends to end
}

/// Request to update a row
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRowRequest {
    // Spec
    #[serde(default)]
    pub spec_content: Option<String>,
    #[serde(default)]
    pub spec_status: Option<SpecStatus>,

    // Task
    #[serde(default)]
    pub task_title: Option<String>,
    #[serde(default)]
    pub task_description: Option<String>,
    #[serde(default)]
    pub task_status: Option<TaskStatus>,
    #[serde(default)]
    pub task_worker: Option<String>,
    #[serde(default)]
    pub task_blocked_by: Option<Vec<String>>,

    // Eval
    #[serde(default)]
    pub eval_criterion: Option<String>,
    #[serde(default)]
    pub eval_status: Option<RowEvalStatus>,
    #[serde(default)]
    pub eval_result: Option<String>,
}

/// Result of dispatch operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DispatchResult {
    Success {
        run_name: String,
    },
    Warning {
        message: String,
        row_ids: Vec<String>,
    },
}

/// Initial task for run dispatch
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialTask {
    pub title: String,
    pub description: Option<String>,
    pub source_island: String,
    pub source_row_id: String,
    pub blocked_by: Vec<String>,
}
