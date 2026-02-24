//! Unified board types
//!
//! Single board_nodes table with kind (feature/task/check) and unified status lifecycle.

use serde::{Deserialize, Serialize};

// =============================================================================
// Node Types
// =============================================================================

/// Kind of a board node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Feature,
    #[default]
    Task,
    Check,
    Plan,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Feature => "feature",
            Self::Task => "task",
            Self::Check => "check",
            Self::Plan => "plan",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "feature" => Self::Feature,
            "check" => Self::Check,
            "plan" => Self::Plan,
            _ => Self::Task,
        }
    }
}

/// Status of a board node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoardNodeStatus {
    #[default]
    Draft,
    Pending,
    Working,
    Done,
    AwaitingCheck,
    Validated,
    NeedsRepair,
    Failed,
}

impl BoardNodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Pending => "pending",
            Self::Working => "working",
            Self::Done => "done",
            Self::AwaitingCheck => "awaiting_check",
            Self::Validated => "validated",
            Self::NeedsRepair => "needs_repair",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "draft" => Self::Draft,
            "working" => Self::Working,
            "done" => Self::Done,
            "awaiting_check" => Self::AwaitingCheck,
            "validated" => Self::Validated,
            "needs_repair" => Self::NeedsRepair,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }

    /// Check if status represents work completion (task output is available).
    /// AwaitingCheck means the worker finished — the check is a separate validation step.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Done | Self::AwaitingCheck | Self::Validated)
    }
}

/// Difficulty of a board node (maps to worker intelligence level)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoardNodeDifficulty {
    Low,
    #[default]
    Medium,
    High,
}

impl BoardNodeDifficulty {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "low" => Self::Low,
            "high" => Self::High,
            _ => Self::Medium,
        }
    }
}

/// Check result for check nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckResult {
    Pass,
    Fail,
}

impl CheckResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            _ => None,
        }
    }
}

/// Source of a board node - where it originated from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoardNodeSource {
    /// Created by user (feature nodes, manual tasks)
    #[default]
    User,
    /// Created by plan worker (tasks decomposed from specs)
    Plan,
    /// Added by worker during execution
    Worker,
    /// System-generated (e.g., plan tasks, repair tasks)
    System,
}

impl BoardNodeSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Plan => "plan",
            Self::Worker => "worker",
            Self::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "plan" => Self::Plan,
            "worker" => Self::Worker,
            "system" => Self::System,
            _ => Self::User,
        }
    }
}

/// A node in the board tree
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardNode {
    pub id: String,
    pub project_id: i64,
    pub parent_id: Option<String>,
    pub position: i32,
    pub name: String,
    pub kind: NodeKind,
    pub source: BoardNodeSource,
    pub content: String,
    #[serde(default)]
    pub difficulty: BoardNodeDifficulty,
    pub status: BoardNodeStatus,
    pub validates: Vec<String>,
    pub validated_by: Vec<String>,
    pub blocked_by: Vec<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub last_commit_sha: Option<String>,
    pub resolves: Option<String>,
    // Orchestration fields
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub completed_by: Option<String>,
    pub check_result: Option<CheckResult>,
    pub check_feedback: Option<String>,
    pub tokens_used: Option<i64>,
}

/// Board node tree (nested for frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardNodeTree {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: NodeKind,
    pub source: BoardNodeSource,
    pub content: String,
    #[serde(default)]
    pub difficulty: BoardNodeDifficulty,
    pub status: BoardNodeStatus,
    pub validates: Vec<String>,
    pub validated_by: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub children: Vec<BoardNodeTree>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub completed_at: Option<String>,
    pub last_commit_sha: Option<String>,
    pub resolves: Option<String>,
    // Orchestration fields
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub completed_by: Option<String>,
    pub check_result: Option<CheckResult>,
    pub check_feedback: Option<String>,
    pub tokens_used: Option<i64>,
}

impl From<BoardNode> for BoardNodeTree {
    fn from(node: BoardNode) -> Self {
        Self {
            id: node.id,
            parent_id: node.parent_id,
            name: node.name,
            kind: node.kind,
            source: node.source,
            content: node.content,
            difficulty: node.difficulty,
            status: node.status,
            validates: node.validates,
            validated_by: node.validated_by,
            blocked_by: node.blocked_by,
            children: vec![],
            x: node.x,
            y: node.y,
            completed_at: node.completed_at,
            last_commit_sha: node.last_commit_sha,
            resolves: node.resolves,
            claimed_by: node.claimed_by,
            claimed_at: node.claimed_at,
            completed_by: node.completed_by,
            check_result: node.check_result,
            check_feedback: node.check_feedback,
            tokens_used: node.tokens_used,
        }
    }
}

// =============================================================================
// Project Run Types
// =============================================================================

/// Status of a project's persistent run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRunStatus {
    #[default]
    Paused,
    Working,
    Failed,
}

impl ProjectRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Working => "working",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "working" => Self::Working,
            "failed" => Self::Failed,
            _ => Self::Paused,
        }
    }
}

/// A persistent run for a project
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRun {
    pub id: i64,
    pub project_id: i64,
    pub route_id: i64,
    pub run_name: String,
    pub status: ProjectRunStatus,
    pub created_at: String,
    pub last_dispatch_at: Option<String>,
}

// =============================================================================
// Board Version Types
// =============================================================================

/// A version of the board (created on each dispatch)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardVersion {
    pub id: i64,
    pub project_id: i64,
    pub version_number: i32,
    pub created_at: String,
    pub description: Option<String>,
}

// =============================================================================
// Delivery Types
// =============================================================================

/// Status of a delivery
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoardDeliveryStatus {
    #[default]
    Pending,
    InProgress,
    ResolvingConflicts,
    Pushed,
    PrOpen,
    Merged,
    Failed,
    Abandoned,
}

impl BoardDeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::ResolvingConflicts => "resolving_conflicts",
            Self::Pushed => "pushed",
            Self::PrOpen => "pr_open",
            Self::Merged => "merged",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "resolving_conflicts" => Self::ResolvingConflicts,
            "pushed" => Self::Pushed,
            "pr_open" => Self::PrOpen,
            "merged" => Self::Merged,
            "failed" => Self::Failed,
            "abandoned" => Self::Abandoned,
            _ => Self::Pending,
        }
    }

    /// Check if delivery is terminal (no further actions possible)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Merged | Self::Abandoned | Self::Failed)
    }
}

/// A delivery tracks the publication of a board version
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub id: i64,
    pub project_id: i64,
    pub route_id: i64,
    pub version_id: i64,
    pub status: BoardDeliveryStatus,
    pub target_branch: String,
    pub delivery_branch: Option<String>,
    pub pr_url: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
}

/// Status of a delivery attempt
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryAttemptStatus {
    Success,
    Failed,
    Cancelled,
}

impl DeliveryAttemptStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "success" => Self::Success,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

/// A delivery attempt (retry history)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryAttempt {
    pub id: i64,
    pub delivery_id: i64,
    pub attempt_number: i32,
    pub status: DeliveryAttemptStatus,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
}

// =============================================================================
// Request Types
// =============================================================================

/// Request to create a board node
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardNodeRequest {
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub kind: NodeKind,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub difficulty: BoardNodeDifficulty,
    #[serde(default)]
    pub validated_by: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// Request to update a board node
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardNodeRequest {
    pub name: Option<String>,
    pub content: Option<String>,
    pub difficulty: Option<BoardNodeDifficulty>,
    pub validated_by: Option<Vec<String>>,
    pub blocked_by: Option<Vec<String>>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// Result of a dispatch operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchResult {
    pub run_name: String,
    pub node_count: usize,
    pub feature_count: usize,
    pub plan_task_count: usize,
    pub version_number: i32,
    pub version_id: i64,
}
