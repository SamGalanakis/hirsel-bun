//! State types and enums
//!
//! All data structures used by SQLiteState.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

// =============================================================================
// Status Enums
// =============================================================================

/// Run status - the overall state of a hirsel run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Draft,     // Configured but not started (no workers spawned)
    Working,   // Workers actively running
    Paused,    // Manually paused by user
    Failed,    // Run failed (see failure_reason for why)
    Eval,      // Evaluation in progress
    Done,      // All work complete, eval passed (or no eval)
    Delivered, // Changes delivered to branch
}

/// Reason for a run failure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    IterationLimit, // was Runaway - max iterations exceeded
    TimeLimit,      // was TimedOut - time limit exceeded
    EvalFailed,     // was EvalFailed - max eval attempts reached
    Manual,         // User manually marked as failed
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Working => "working",
            Status::Paused => "paused",
            Status::Failed => "failed",
            Status::Eval => "eval",
            Status::Done => "done",
            Status::Delivered => "delivered",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Status::Draft),
            "working" => Some(Status::Working),
            "paused" => Some(Status::Paused),
            "failed" => Some(Status::Failed),
            "eval" => Some(Status::Eval),
            "done" => Some(Status::Done),
            "delivered" => Some(Status::Delivered),
            _ => None,
        }
    }

    /// Check if this status represents a completed run (terminal state)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Status::Done | Status::Delivered | Status::Failed)
    }

    /// Check if this status allows workers to run
    pub fn is_active(&self) -> bool {
        matches!(self, Status::Working | Status::Eval)
    }
}

impl FailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureReason::IterationLimit => "iteration_limit",
            FailureReason::TimeLimit => "time_limit",
            FailureReason::EvalFailed => "eval_failed",
            FailureReason::Manual => "manual",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "iteration_limit" => Some(FailureReason::IterationLimit),
            "time_limit" => Some(FailureReason::TimeLimit),
            "eval_failed" => Some(FailureReason::EvalFailed),
            "manual" => Some(FailureReason::Manual),
            _ => None,
        }
    }

    pub fn display_message(&self) -> &'static str {
        match self {
            FailureReason::IterationLimit => "Maximum iterations exceeded",
            FailureReason::TimeLimit => "Time limit reached",
            FailureReason::EvalFailed => "Evaluation failed after max retries",
            FailureReason::Manual => "Manually marked as failed",
        }
    }
}

impl std::fmt::Display for FailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Eval status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalStatus {
    Running,
    Passed,
    Failed,
}

impl EvalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvalStatus::Running => "running",
            EvalStatus::Passed => "passed",
            EvalStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(EvalStatus::Running),
            "passed" => Some(EvalStatus::Passed),
            "failed" => Some(EvalStatus::Failed),
            _ => None,
        }
    }
}

impl std::fmt::Display for EvalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Worker status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Working,  // Actively processing a task
    Awaiting, // No work to do OR waiting for user (check hitl_waiting flag)
    Paused,   // Worker paused (run is paused/failed)
    Error,    // Worker process died unexpectedly
}

impl WorkerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerStatus::Working => "working",
            WorkerStatus::Awaiting => "awaiting",
            WorkerStatus::Paused => "paused",
            WorkerStatus::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "working" => Some(WorkerStatus::Working),
            "awaiting" => Some(WorkerStatus::Awaiting),
            "paused" => Some(WorkerStatus::Paused),
            "error" => Some(WorkerStatus::Error),
            _ => None,
        }
    }

    /// Check if worker is inactive (not actively working)
    pub fn is_inactive(&self) -> bool {
        matches!(self, WorkerStatus::Awaiting | WorkerStatus::Error)
    }

    /// Check if worker can be resumed
    pub fn can_resume(&self) -> bool {
        matches!(
            self,
            WorkerStatus::Paused | WorkerStatus::Error | WorkerStatus::Awaiting
        )
    }
}

impl std::fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Eval result - pass or fail
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalResult {
    Pass,
    Fail,
}

impl EvalResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvalResult::Pass => "pass",
            EvalResult::Fail => "fail",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(EvalResult::Pass),
            "fail" => Some(EvalResult::Fail),
            _ => None,
        }
    }
}

impl std::fmt::Display for EvalResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Delivery status - tracks the publication state of a run's changes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,   // Not yet delivered
    Pushed,    // Branch on remote, no PR
    PrOpen,    // PR created
    Merged,    // Merged to target
    Abandoned, // Discarded
}

impl DeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeliveryStatus::Pending => "pending",
            DeliveryStatus::Pushed => "pushed",
            DeliveryStatus::PrOpen => "pr_open",
            DeliveryStatus::Merged => "merged",
            DeliveryStatus::Abandoned => "abandoned",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(DeliveryStatus::Pending),
            "pushed" => Some(DeliveryStatus::Pushed),
            "pr_open" => Some(DeliveryStatus::PrOpen),
            "merged" => Some(DeliveryStatus::Merged),
            "abandoned" => Some(DeliveryStatus::Abandoned),
            _ => None,
        }
    }

    /// Check if delivery is terminal (no further actions possible)
    pub fn is_terminal(&self) -> bool {
        matches!(self, DeliveryStatus::Merged | DeliveryStatus::Abandoned)
    }
}

impl Default for DeliveryStatus {
    fn default() -> Self {
        DeliveryStatus::Pending
    }
}

impl std::fmt::Display for DeliveryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Merge state - tracks whether a run can be cleanly merged
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeState {
    Unknown,   // Not yet checked
    Clean,     // Auto-merge possible
    Conflicts, // Needs resolution
}

impl MergeState {
    pub fn as_str(&self) -> &'static str {
        match self {
            MergeState::Unknown => "unknown",
            MergeState::Clean => "clean",
            MergeState::Conflicts => "conflicts",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "unknown" => Some(MergeState::Unknown),
            "clean" => Some(MergeState::Clean),
            "conflicts" => Some(MergeState::Conflicts),
            _ => None,
        }
    }
}

impl Default for MergeState {
    fn default() -> Self {
        MergeState::Unknown
    }
}

impl std::fmt::Display for MergeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Type of worker output event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerEventType {
    /// Agent text output
    Text,
    /// Tool call started
    ToolStart,
    /// Tool call status update
    ToolUpdate,
    /// Agent thought/reasoning (if enabled)
    Thought,
}

impl WorkerEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerEventType::Text => "text",
            WorkerEventType::ToolStart => "tool_start",
            WorkerEventType::ToolUpdate => "tool_update",
            WorkerEventType::Thought => "thought",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "text" => Some(WorkerEventType::Text),
            "tool_start" => Some(WorkerEventType::ToolStart),
            "tool_update" => Some(WorkerEventType::ToolUpdate),
            "thought" => Some(WorkerEventType::Thought),
            _ => None,
        }
    }
}

/// Tool call status (from ACP)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolCallStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolCallStatus::Pending => "pending",
            ToolCallStatus::InProgress => "in_progress",
            ToolCallStatus::Completed => "completed",
            ToolCallStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(ToolCallStatus::Pending),
            "in_progress" => Some(ToolCallStatus::InProgress),
            "completed" => Some(ToolCallStatus::Completed),
            "failed" => Some(ToolCallStatus::Failed),
            _ => None,
        }
    }
}

// =============================================================================
// Data Structures
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: i64,
    pub name: String,
    pub pid: Option<i64>,
    pub runner_id: Option<String>,
    pub runner_type: Option<String>,
    pub session_id: Option<String>,
    pub session_started_at: Option<String>,
    pub status: WorkerStatus,
    pub work_dir: Option<String>,
    pub waiting_thread: Option<String>,
    pub needs_restart: bool,
    pub location: String,
    pub last_heartbeat: Option<String>,
    pub created_at: String,
    pub hitl_waiting: bool, // True if worker is awaiting user input (HITL)
    pub state_handle: Option<String>, // JSON-serialized WorkerStateHandle for pause/resume
    // Direct task assignment fields
    pub assigned_task_id: Option<String>, // Currently assigned task
    pub last_task_id: Option<String>,     // Last completed task (for tree distance)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eval {
    pub id: i64,
    pub branch: String,
    pub eval_name: Option<String>,
    pub status: EvalStatus,
    pub feedback: Option<String>,
    pub log_file: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub timestamp: String,
    pub action: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeInfo {
    pub limit_minutes: i64,
    pub started_at: DateTime<Local>,
    pub elapsed_minutes: f64,
    pub remaining_minutes: f64,
    pub percent_elapsed: f64,
    pub percent_remaining: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Amendment {
    pub id: i64,
    pub message: String,
    pub timestamp: String,
    pub author: String,
    pub spec_hash: String,
}

/// Summary data for displaying a run in a list (optimized fetch)
/// Note: Task counts come from DeltaState.live_nodes for project runs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStateSummary {
    pub status: Status,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<String>,
    pub time_limit_minutes: Option<i64>,
    pub unread_count: i64,
    pub workers_active: u32,
    pub workers_total: u32,
    pub elapsed_minutes: f64,
}

/// Worker output event for real-time streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEvent {
    pub id: i64,
    pub worker_name: String,
    pub event_type: WorkerEventType,
    pub timestamp: String,
    /// Text content (for Text/Thought events)
    pub content: Option<String>,
    /// Tool call ID (for tool events)
    pub tool_call_id: Option<String>,
    /// Tool title/name
    pub tool_title: Option<String>,
    /// Tool kind (read, edit, execute, search, etc.)
    pub tool_kind: Option<String>,
    /// Tool execution status
    pub tool_status: Option<ToolCallStatus>,
    /// Tool input (JSON)
    pub tool_input: Option<String>,
    /// Tool output (JSON)
    pub tool_output: Option<String>,
}

// =============================================================================
// Error Type
// =============================================================================

#[derive(Debug)]
pub enum StateError {
    Database(sqlx::Error),
    NotFound(String),
    InvalidState(String),
    AlreadyExists(String),
    Blocked(String),
    InvalidTransition(Status, Status),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Database(e) => write!(f, "Database error: {}", e),
            StateError::NotFound(s) => write!(f, "Not found: {}", s),
            StateError::InvalidState(s) => write!(f, "Invalid state: {}", s),
            StateError::AlreadyExists(s) => write!(f, "Already exists: {}", s),
            StateError::Blocked(s) => write!(f, "Blocked: {}", s),
            StateError::InvalidTransition(from, to) => {
                write!(f, "Invalid transition: {} -> {}", from, to)
            }
        }
    }
}

impl std::error::Error for StateError {}

impl From<sqlx::Error> for StateError {
    fn from(e: sqlx::Error) -> Self {
        StateError::Database(e)
    }
}

pub type StateResult<T> = Result<T, StateError>;

// =============================================================================
// Worker Update Helper
// =============================================================================

/// Helper struct for partial worker updates
#[derive(Default)]
pub struct WorkerUpdate {
    pub pid: Option<i64>,
    pub runner_id: Option<String>,
    pub runner_type: Option<String>,
    pub session_id: Option<String>,
    pub status: Option<WorkerStatus>,
    pub waiting_thread: Option<String>,
    pub needs_restart: Option<bool>,
    pub last_heartbeat: Option<String>,
    pub hitl_waiting: Option<bool>,
    /// Worker state handle (JSON-serialized). Use Some(Some(json)) to set, Some(None) to clear.
    pub state_handle: Option<Option<String>>,
    /// Currently assigned task. Use Some(Some(id)) to set, Some(None) to clear.
    pub assigned_task_id: Option<Option<String>>,
    /// Last completed task (for tree distance). Use Some(Some(id)) to set, Some(None) to clear.
    pub last_task_id: Option<Option<String>>,
}
