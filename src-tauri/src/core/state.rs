//! SQLite state management for Hirsel runs
//!
//! This module provides the core state management functionality for tracking
//! runs, tasks, workers, evals, and messages.

use chrono::{DateTime, Local, Utc};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// =============================================================================
// Status Enums
// =============================================================================

/// Run status - the overall state of a hirsel run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Working,
    Paused,      // Manually paused by user
    Runaway,     // Auto-paused due to max_iterations exceeded
    TimedOut,    // Auto-paused due to time limit exceeded
    Eval,
    EvalFailed,  // Max eval attempts reached
    Waiting,
    Done,
    Delivered,
    Merged,      // Work merged to staging/main
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Working => "working",
            Status::Paused => "paused",
            Status::Runaway => "runaway",
            Status::TimedOut => "timed_out",
            Status::Eval => "eval",
            Status::EvalFailed => "eval_failed",
            Status::Waiting => "waiting",
            Status::Done => "done",
            Status::Delivered => "delivered",
            Status::Merged => "merged",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Status::Idle),
            "working" => Some(Status::Working),
            "paused" => Some(Status::Paused),
            "runaway" => Some(Status::Runaway),
            "timed_out" => Some(Status::TimedOut),
            "eval" => Some(Status::Eval),
            "eval_failed" => Some(Status::EvalFailed),
            "waiting" => Some(Status::Waiting),
            "done" => Some(Status::Done),
            "delivered" => Some(Status::Delivered),
            "merged" => Some(Status::Merged),
            _ => None,
        }
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
    Idle,
    Working,
    Waiting,   // Waiting for user reply
    Awaiting,  // No work to do (no tasks available OR all work complete)
    Paused,    // Worker paused (run is paused/runaway)
    Error,     // Worker process died unexpectedly
}

impl WorkerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerStatus::Idle => "idle",
            WorkerStatus::Working => "working",
            WorkerStatus::Waiting => "waiting",
            WorkerStatus::Awaiting => "awaiting",
            WorkerStatus::Paused => "paused",
            WorkerStatus::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(WorkerStatus::Idle),
            "working" => Some(WorkerStatus::Working),
            "waiting" => Some(WorkerStatus::Waiting),
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
}

impl std::fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::Doing => "doing",
            TaskStatus::Done => "done",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(TaskStatus::Todo),
            "doing" => Some(TaskStatus::Doing),
            "done" => Some(TaskStatus::Done),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// Data Structures
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub pending_done_at: Option<String>,
    pub tokens_used: Option<i64>,
    pub parent_id: Option<String>,
    pub blocked_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: i64,
    pub name: String,
    pub pid: Option<i64>,
    pub session_id: Option<String>,
    pub session_started_at: Option<String>,
    pub status: WorkerStatus,
    pub work_dir: Option<String>,
    pub waiting_thread: Option<String>,
    pub needs_restart: bool,
    pub location: String,
    pub last_heartbeat: Option<String>,
    pub created_at: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub thread: String,
    pub sender: String,
    pub content: String,
    pub timestamp: String,
    pub waiting: bool,
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

/// Worker output event for real-time streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Sqlite(rusqlite::Error),
    NotFound(String),
    InvalidState(String),
    AlreadyExists(String),
    Blocked(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Sqlite(e) => write!(f, "SQLite error: {}", e),
            StateError::NotFound(s) => write!(f, "Not found: {}", s),
            StateError::InvalidState(s) => write!(f, "Invalid state: {}", s),
            StateError::AlreadyExists(s) => write!(f, "Already exists: {}", s),
            StateError::Blocked(s) => write!(f, "Blocked: {}", s),
        }
    }
}

impl std::error::Error for StateError {}

impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self {
        StateError::Sqlite(e)
    }
}

pub type StateResult<T> = Result<T, StateError>;

// =============================================================================
// Schema
// =============================================================================

const SCHEMA: &str = r#"
-- hirsel run state schema

CREATE TABLE IF NOT EXISTS state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    status TEXT NOT NULL DEFAULT 'idle',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    request TEXT,
    project_path TEXT,
    unread_count INTEGER DEFAULT 0,
    human_in_the_loop INTEGER DEFAULT 1,
    summary TEXT,
    waiting_reason TEXT,
    worker_scale TEXT,
    time_limit_minutes INTEGER,
    started_at TEXT,
    last_time_notification_pct INTEGER,
    learnings_processed_at TEXT,
    iteration_count INTEGER DEFAULT 0,
    max_iterations INTEGER,
    pause_mode TEXT DEFAULT 'sender'
);

CREATE TABLE IF NOT EXISTS workers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    pid INTEGER,
    session_id TEXT,
    session_started_at TEXT,
    status TEXT NOT NULL DEFAULT 'idle',
    work_dir TEXT,
    waiting_thread TEXT,
    needs_restart INTEGER DEFAULT 0,
    location TEXT DEFAULT 'local',
    last_heartbeat TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    action TEXT NOT NULL,
    detail TEXT
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'todo',
    created_at TEXT NOT NULL,
    completed_at TEXT,
    claimed_by TEXT,
    claimed_at TEXT,
    pending_done_at TEXT,
    tokens_used INTEGER,
    parent_id TEXT,
    blocked_by TEXT
);

CREATE TABLE IF NOT EXISTS evals (
    id INTEGER PRIMARY KEY,
    branch TEXT NOT NULL,
    eval_name TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    feedback TEXT,
    log_file TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    thread TEXT NOT NULL,
    sender TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    waiting INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS message_reads (
    worker_name TEXT NOT NULL,
    thread TEXT NOT NULL,
    last_read_id INTEGER NOT NULL,
    PRIMARY KEY (worker_name, thread)
);

CREATE TABLE IF NOT EXISTS amendments (
    id INTEGER PRIMARY KEY,
    message TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    author TEXT NOT NULL DEFAULT 'user',
    spec_hash TEXT NOT NULL
);

-- Worker output events for real-time UI streaming
-- event_type: 'text', 'tool_start', 'tool_update', 'tool_end', 'thought'
-- tool_status: 'pending', 'in_progress', 'completed', 'failed' (for tool events)
CREATE TABLE IF NOT EXISTS worker_events (
    id INTEGER PRIMARY KEY,
    worker_name TEXT NOT NULL,
    event_type TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    -- For text/thought events
    content TEXT,
    -- For tool events
    tool_call_id TEXT,
    tool_title TEXT,
    tool_kind TEXT,
    tool_status TEXT,
    tool_input TEXT,
    tool_output TEXT
);

CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread);
CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp);
CREATE INDEX IF NOT EXISTS idx_worker_events_worker ON worker_events(worker_name);
CREATE INDEX IF NOT EXISTS idx_worker_events_timestamp ON worker_events(timestamp);
"#;

// =============================================================================
// SQLiteState Implementation
// =============================================================================

/// SQLite-backed state management for a hirsel run
pub struct SQLiteState {
    db: Connection,
    db_path: PathBuf,
}

impl SQLiteState {
    /// Create a new SQLiteState, opening or creating the database at the given path
    pub fn new(db_path: PathBuf) -> StateResult<Self> {
        let db = Connection::open(&db_path)?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;

        let mut state = Self { db, db_path };
        state.init_db()?;
        Ok(state)
    }

    /// Reconnect to the database (useful for getting fresh data)
    pub fn reconnect(&mut self) -> StateResult<()> {
        self.db = Connection::open(&self.db_path)?;
        self.db.busy_timeout(std::time::Duration::from_secs(30))?;
        Ok(())
    }

    fn init_db(&mut self) -> StateResult<()> {
        self.db.execute_batch(SCHEMA)?;
        Ok(())
    }

    fn now(&self) -> String {
        Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    }

    fn log_history(&self, action: &str, detail: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "INSERT INTO history (timestamp, action, detail) VALUES (?1, ?2, ?3)",
            params![self.now(), action, detail],
        )?;
        Ok(())
    }

    // =========================================================================
    // Run State Methods
    // =========================================================================

    /// Get the current run status
    pub fn status(&self) -> StateResult<Status> {
        let status: String = self.db.query_row(
            "SELECT status FROM state WHERE id = 1",
            [],
            |row| row.get(0),
        ).unwrap_or_else(|_| "idle".to_string());

        Ok(Status::from_str(&status).unwrap_or(Status::Idle))
    }

    /// Set the run status
    pub fn set_status(&self, status: Status) -> StateResult<()> {
        let old_status = self.status()?;
        if old_status == status {
            return Ok(()); // No change
        }

        let now = self.now();
        self.db.execute(
            r#"
            INSERT INTO state (id, status, created_at, updated_at)
            VALUES (1, ?1, ?2, ?2)
            ON CONFLICT(id) DO UPDATE SET status = ?1, updated_at = ?2
            "#,
            params![status.as_str(), now],
        )?;

        self.log_history("status_change", Some(&format!("run {}", status)))?;
        Ok(())
    }

    /// Initialize the state for a new run
    pub fn init_state(&self, project_path: Option<&str>) -> StateResult<()> {
        let now = self.now();
        self.db.execute(
            r#"
            INSERT OR REPLACE INTO state (id, status, created_at, updated_at, project_path)
            VALUES (1, ?1, ?2, ?2, ?3)
            "#,
            params![Status::Idle.as_str(), now, project_path],
        )?;
        self.log_history("init", None)?;
        Ok(())
    }

    /// Get the request text
    pub fn get_request(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT request FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the request text
    pub fn set_request(&self, request: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET request = ?1, updated_at = ?2 WHERE id = 1",
            params![request, self.now()],
        )?;
        Ok(())
    }

    /// Get the waiting reason
    pub fn get_waiting_reason(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT waiting_reason FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the waiting reason
    pub fn set_waiting_reason(&self, reason: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET waiting_reason = ?1, updated_at = ?2 WHERE id = 1",
            params![reason, self.now()],
        )?;
        Ok(())
    }

    /// Get project path
    pub fn get_project_path(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT project_path FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set project path
    pub fn set_project_path(&self, path: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET project_path = ?1, updated_at = ?2 WHERE id = 1",
            params![path, self.now()],
        )?;
        Ok(())
    }

    /// Get created_at timestamp
    pub fn get_created_at(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT created_at FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get summary
    pub fn get_summary(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT summary FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set summary
    pub fn set_summary(&self, summary: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET summary = ?1, updated_at = ?2 WHERE id = 1",
            params![summary, self.now()],
        )?;
        self.log_history("summary_generated", None)?;
        Ok(())
    }

    /// Get worker scale
    pub fn get_worker_scale(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT worker_scale FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set worker scale
    pub fn set_worker_scale(&self, scale: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET worker_scale = ?1, updated_at = ?2 WHERE id = 1",
            params![scale, self.now()],
        )?;
        Ok(())
    }

    /// Get human in the loop setting
    pub fn get_human_in_the_loop(&self) -> StateResult<bool> {
        match self.db.query_row(
            "SELECT human_in_the_loop FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(Some(val)) => Ok(val != 0),
            Ok(None) => Ok(true), // Default to HITL
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set human in the loop
    pub fn set_human_in_the_loop(&self, enabled: bool) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET human_in_the_loop = ?1, updated_at = ?2 WHERE id = 1",
            params![if enabled { 1 } else { 0 }, self.now()],
        )?;
        self.log_history("mode_change", Some(if enabled { "hitl" } else { "yolo" }))?;

        // Notify workers via group chat
        let msg = if enabled {
            "The user is now available. Feel free to message them if needed."
        } else {
            "The user is currently unavailable for messages. Do not attempt to contact them - do the work to the best of your abilities."
        };
        self.add_message("group", "System", msg, false)?;
        Ok(())
    }

    // =========================================================================
    // Time Tracking Methods
    // =========================================================================

    /// Get time limit in minutes
    pub fn get_time_limit_minutes(&self) -> StateResult<Option<i64>> {
        match self.db.query_row(
            "SELECT time_limit_minutes FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set time limit in minutes
    pub fn set_time_limit_minutes(&self, minutes: Option<i64>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET time_limit_minutes = ?1, updated_at = ?2 WHERE id = 1",
            params![minutes, self.now()],
        )?;
        Ok(())
    }

    /// Get started_at timestamp
    pub fn get_started_at(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT started_at FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set started_at timestamp
    pub fn set_started_at(&self, timestamp: Option<&str>) -> StateResult<()> {
        let ts = timestamp.map(|s| s.to_string()).unwrap_or_else(|| self.now());
        self.db.execute(
            "UPDATE state SET started_at = ?1, updated_at = ?2 WHERE id = 1",
            params![ts, self.now()],
        )?;
        Ok(())
    }

    /// Clear time tracking
    pub fn clear_time_tracking(&self) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET started_at = NULL, last_time_notification_pct = NULL, updated_at = ?1 WHERE id = 1",
            params![self.now()],
        )?;
        Ok(())
    }

    /// Get last time notification percentage
    pub fn get_last_time_notification_pct(&self) -> StateResult<Option<i64>> {
        match self.db.query_row(
            "SELECT last_time_notification_pct FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set last time notification percentage
    pub fn set_last_time_notification_pct(&self, pct: i64) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET last_time_notification_pct = ?1, updated_at = ?2 WHERE id = 1",
            params![pct, self.now()],
        )?;
        Ok(())
    }

    /// Get time info
    pub fn get_time_info(&self) -> StateResult<Option<TimeInfo>> {
        let row: Option<(Option<i64>, Option<String>)> = self.db.query_row(
            "SELECT time_limit_minutes, started_at FROM state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok();

        match row {
            Some((Some(limit_minutes), Some(started_at_str))) => {
                let started_at = DateTime::parse_from_rfc3339(&started_at_str)
                    .or_else(|_| DateTime::parse_from_str(&started_at_str, "%Y-%m-%dT%H:%M:%S%.f"))
                    .map(|dt| dt.with_timezone(&Local))
                    .map_err(|_| StateError::InvalidState("Invalid started_at timestamp".to_string()))?;

                let now = Local::now();
                let elapsed = now.signed_duration_since(started_at);
                let elapsed_minutes = elapsed.num_seconds() as f64 / 60.0;
                let remaining_minutes = (limit_minutes as f64 - elapsed_minutes).max(0.0);
                let percent_elapsed = ((elapsed_minutes / limit_minutes as f64) * 100.0).min(100.0);
                let percent_remaining = (100.0 - percent_elapsed).max(0.0);

                Ok(Some(TimeInfo {
                    limit_minutes,
                    started_at,
                    elapsed_minutes,
                    remaining_minutes,
                    percent_elapsed,
                    percent_remaining,
                }))
            }
            _ => Ok(None),
        }
    }

    /// Check if time has expired
    pub fn is_time_expired(&self) -> StateResult<bool> {
        match self.get_time_info()? {
            Some(info) => Ok(info.remaining_minutes <= 0.0),
            None => Ok(false),
        }
    }

    // =========================================================================
    // Iteration Tracking
    // =========================================================================

    /// Get iteration count
    pub fn get_iteration_count(&self) -> StateResult<i64> {
        match self.db.query_row(
            "SELECT iteration_count FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(Some(val)) => Ok(val),
            Ok(None) => Ok(0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Increment iteration count
    pub fn increment_iteration(&self) -> StateResult<i64> {
        self.db.execute(
            "UPDATE state SET iteration_count = COALESCE(iteration_count, 0) + 1, updated_at = ?1 WHERE id = 1",
            params![self.now()],
        )?;
        self.get_iteration_count()
    }

    /// Get max iterations
    pub fn get_max_iterations(&self) -> StateResult<Option<i64>> {
        match self.db.query_row(
            "SELECT max_iterations FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set max iterations
    pub fn set_max_iterations(&self, max_iter: Option<i64>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET max_iterations = ?1, updated_at = ?2 WHERE id = 1",
            params![max_iter, self.now()],
        )?;
        Ok(())
    }

    /// Get learnings processed at timestamp
    pub fn get_learnings_processed_at(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT learnings_processed_at FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set learnings processed at timestamp
    pub fn set_learnings_processed_at(&self, timestamp: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET learnings_processed_at = ?1, updated_at = ?2 WHERE id = 1",
            params![timestamp, self.now()],
        )?;
        Ok(())
    }

    /// Get pause mode ("sender" or "all")
    pub fn get_pause_mode(&self) -> StateResult<String> {
        match self.db.query_row(
            "SELECT pause_mode FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(Some(val)) => Ok(val),
            Ok(None) => Ok("sender".to_string()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok("sender".to_string()),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set pause mode ("sender" or "all")
    pub fn set_pause_mode(&self, mode: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET pause_mode = ?1, updated_at = ?2 WHERE id = 1",
            params![mode, self.now()],
        )?;
        Ok(())
    }

    // =========================================================================
    // Task Methods
    // =========================================================================

    fn task_from_row(row: &Row) -> rusqlite::Result<Task> {
        Ok(Task {
            id: row.get("id")?,
            name: row.get("name")?,
            status: TaskStatus::from_str(&row.get::<_, String>("status")?).unwrap_or(TaskStatus::Todo),
            created_at: row.get("created_at")?,
            completed_at: row.get("completed_at")?,
            claimed_by: row.get("claimed_by")?,
            claimed_at: row.get("claimed_at")?,
            pending_done_at: row.get("pending_done_at")?,
            tokens_used: row.get("tokens_used")?,
            parent_id: row.get("parent_id")?,
            blocked_by: row.get("blocked_by")?,
        })
    }

    /// Get task depth in hierarchy
    pub fn get_task_depth(&self, task_id: &str) -> StateResult<usize> {
        let mut depth = 0;
        let mut current_id = Some(task_id.to_string());

        while let Some(id) = current_id {
            if let Some(task) = self.get_task(&id)? {
                depth += 1;
                current_id = task.parent_id;
            } else {
                break;
            }
        }
        Ok(depth)
    }

    /// Get children of a task
    pub fn get_children(&self, task_id: &str) -> StateResult<Vec<Task>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks WHERE parent_id = ?1 ORDER BY created_at"
        )?;
        let tasks = stmt.query_map(params![task_id], Self::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// Check if task has children
    pub fn has_children(&self, task_id: &str) -> StateResult<bool> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM tasks WHERE parent_id = ?1",
            params![task_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Add a new task
    pub fn add_task(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
    ) -> StateResult<()> {
        // Check hierarchy depth limit (max 3 levels)
        if let Some(pid) = parent_id {
            let parent_depth = self.get_task_depth(pid)?;
            if parent_depth == 0 {
                return Err(StateError::NotFound(format!("Parent task '{}' not found", pid)));
            }
            if parent_depth >= 3 {
                return Err(StateError::InvalidState(format!(
                    "Cannot add child to '{}': max hierarchy depth is 3",
                    pid
                )));
            }
        }

        let blocked_by_str = blocked_by.map(|b| b.join(","));

        match self.db.execute(
            "INSERT INTO tasks (id, name, status, created_at, parent_id, blocked_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![task_id, name, TaskStatus::Todo.as_str(), self.now(), parent_id, blocked_by_str],
        ) {
            Ok(_) => {
                let mut detail = format!("{}: {}", task_id, name);
                if let Some(pid) = parent_id {
                    detail.push_str(&format!(" (parent: {})", pid));
                }
                if let Some(b) = blocked_by {
                    detail.push_str(&format!(" (blocked by: {})", b.join(", ")));
                }
                self.log_history("task_add", Some(&detail))?;
                Ok(())
            }
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.extended_code == 1555 => {
                Err(StateError::AlreadyExists(format!("Task '{}' already exists", task_id)))
            }
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get all tasks
    pub fn get_tasks(&self) -> StateResult<Vec<Task>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks ORDER BY created_at"
        )?;
        let tasks = stmt.query_map([], Self::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// Get a specific task
    pub fn get_task(&self, task_id: &str) -> StateResult<Option<Task>> {
        let result = self.db.query_row(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks WHERE id = ?1",
            params![task_id],
            Self::task_from_row,
        );
        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Check if a task is blocked
    pub fn is_task_blocked(&self, task_id: &str) -> StateResult<bool> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Ok(false),
        };

        let blocked_by = match &task.blocked_by {
            Some(b) if !b.is_empty() => b,
            _ => return Ok(false),
        };

        for blocker_id in blocked_by.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if let Some(blocker) = self.get_task(blocker_id)? {
                if blocker.status != TaskStatus::Done {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Get incomplete blockers for a task
    pub fn get_blockers(&self, task_id: &str) -> StateResult<Vec<String>> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let blocked_by = match &task.blocked_by {
            Some(b) if !b.is_empty() => b,
            _ => return Ok(vec![]),
        };

        let mut incomplete = vec![];
        for blocker_id in blocked_by.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if let Some(blocker) = self.get_task(blocker_id)? {
                if blocker.status != TaskStatus::Done {
                    incomplete.push(blocker_id.to_string());
                }
            }
        }
        Ok(incomplete)
    }

    /// Get tasks that can be claimed
    pub fn get_claimable_tasks(&self) -> StateResult<Vec<Task>> {
        let tasks = self.get_tasks()?;
        let mut claimable = vec![];

        for task in tasks {
            if task.status != TaskStatus::Todo {
                continue;
            }
            if task.claimed_by.is_some() {
                continue;
            }
            if task.id == "scope" {
                continue;
            }
            if self.is_task_blocked(&task.id)? {
                continue;
            }
            claimable.push(task);
        }
        Ok(claimable)
    }

    /// Claim a task for a worker
    pub fn claim_task(&self, task_id: &str, worker_name: &str) -> StateResult<()> {
        // Use BEGIN IMMEDIATE to prevent race conditions
        self.db.execute("BEGIN IMMEDIATE", [])?;

        let result = (|| -> StateResult<()> {
            let task = match self.get_task(task_id)? {
                Some(t) => t,
                None => return Err(StateError::NotFound(format!("Task '{}' not found", task_id))),
            };

            if task.status != TaskStatus::Todo {
                return Err(StateError::InvalidState(format!(
                    "Task '{}' is not TODO (status: {})",
                    task_id, task.status
                )));
            }

            if self.has_children(task_id)? {
                return Err(StateError::InvalidState(format!(
                    "Task '{}' has children and cannot be claimed directly",
                    task_id
                )));
            }

            // Check if worker already has a claimed task
            let existing: Option<String> = self.db.query_row(
                "SELECT id FROM tasks WHERE claimed_by = ?1 AND status = ?2",
                params![worker_name, TaskStatus::Doing.as_str()],
                |row| row.get(0),
            ).ok();

            if let Some(existing_id) = existing {
                return Err(StateError::InvalidState(format!(
                    "You already have task '{}' claimed. Complete or unclaim it first.",
                    existing_id
                )));
            }

            self.db.execute(
                "UPDATE tasks SET status = ?1, claimed_by = ?2, claimed_at = ?3 WHERE id = ?4",
                params![TaskStatus::Doing.as_str(), worker_name, self.now(), task_id],
            )?;

            self.log_history("task_claim", Some(&format!("{} by {}", task_id, worker_name)))?;
            Ok(())
        })();

        match result {
            Ok(_) => {
                self.db.execute("COMMIT", [])?;
                Ok(())
            }
            Err(e) => {
                let _ = self.db.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Unclaim a task
    pub fn unclaim_task(&self, task_id: &str, worker_name: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Err(StateError::NotFound(format!("Task '{}' not found", task_id))),
        };

        if task.status != TaskStatus::Doing || task.claimed_by.as_deref() != Some(worker_name) {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not claimed by {}",
                task_id, worker_name
            )));
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, claimed_by = NULL, claimed_at = NULL WHERE id = ?2",
            params![TaskStatus::Todo.as_str(), task_id],
        )?;

        self.log_history("task_unclaim", Some(&format!("{} by {}", task_id, worker_name)))?;
        Ok(())
    }

    /// Complete a task
    pub fn complete_task(&self, task_id: &str, worker_name: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Err(StateError::NotFound(format!("Task '{}' not found", task_id))),
        };

        if task.status != TaskStatus::Doing || task.claimed_by.as_deref() != Some(worker_name) {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not claimed by {}",
                task_id, worker_name
            )));
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![TaskStatus::Done.as_str(), self.now(), task_id],
        )?;

        self.log_history("task_done", Some(&format!("{} by {}", task_id, worker_name)))?;

        // Auto-complete parent if all siblings are done
        if let Some(parent_id) = &task.parent_id {
            self.maybe_complete_parent(parent_id)?;
        }

        Ok(())
    }

    fn maybe_complete_parent(&self, parent_id: &str) -> StateResult<()> {
        let children = self.get_children(parent_id)?;
        if children.is_empty() {
            return Ok(());
        }

        let all_done = children.iter().all(|c| c.status == TaskStatus::Done);
        if !all_done {
            return Ok(());
        }

        let parent = match self.get_task(parent_id)? {
            Some(p) => p,
            None => return Ok(()),
        };

        if parent.status == TaskStatus::Done {
            return Ok(());
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![TaskStatus::Done.as_str(), self.now(), parent_id],
        )?;

        self.log_history("task_done", Some(&format!("{} (auto-completed)", parent_id)))?;

        // Recursively check grandparent
        if let Some(grandparent_id) = &parent.parent_id {
            self.maybe_complete_parent(grandparent_id)?;
        }

        Ok(())
    }

    /// Reopen a completed task
    pub fn reopen_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Err(StateError::NotFound(format!("Task '{}' not found", task_id))),
        };

        if task.status != TaskStatus::Done {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not done",
                task_id
            )));
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, claimed_by = NULL, claimed_at = NULL, completed_at = NULL WHERE id = ?2",
            params![TaskStatus::Todo.as_str(), task_id],
        )?;

        self.log_history("task_reopen", Some(task_id))?;
        Ok(())
    }

    /// Set pending_done_at for a task (first phase of two-phase completion)
    pub fn set_task_pending_done(&self, task_id: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE tasks SET pending_done_at = ?1 WHERE id = ?2",
            params![self.now(), task_id],
        )?;
        Ok(())
    }

    /// Clear pending_done_at for a task (second phase of two-phase completion)
    pub fn clear_task_pending_done(&self, task_id: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE tasks SET pending_done_at = NULL WHERE id = ?1",
            params![task_id],
        )?;
        Ok(())
    }

    /// Delete a task and all its children
    pub fn delete_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Err(StateError::NotFound(format!("Task '{}' not found", task_id))),
        };

        if task.status == TaskStatus::Doing {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is currently claimed and cannot be deleted",
                task_id
            )));
        }

        // Collect all descendant IDs
        fn collect_descendants(state: &SQLiteState, tid: &str) -> StateResult<Vec<String>> {
            let children = state.get_children(tid)?;
            let mut descendants = vec![];
            for child in children {
                descendants.push(child.id.clone());
                descendants.extend(collect_descendants(state, &child.id)?);
            }
            Ok(descendants)
        }

        let mut all_to_delete = vec![task_id.to_string()];
        all_to_delete.extend(collect_descendants(self, task_id)?);

        // Check if any are claimed
        for tid in &all_to_delete {
            if let Some(t) = self.get_task(tid)? {
                if t.status == TaskStatus::Doing {
                    return Err(StateError::InvalidState(format!(
                        "Child task '{}' is currently claimed and cannot be deleted",
                        tid
                    )));
                }
            }
        }

        // Remove from blocked_by lists of other tasks
        let all_tasks = self.get_tasks()?;
        for t in all_tasks {
            if let Some(blocked_by) = &t.blocked_by {
                let blockers: Vec<&str> = blocked_by.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                let new_blockers: Vec<&str> = blockers.into_iter()
                    .filter(|b| !all_to_delete.contains(&b.to_string()))
                    .collect();
                if new_blockers.len() != blocked_by.split(',').count() {
                    let new_blocked_by = if new_blockers.is_empty() {
                        None
                    } else {
                        Some(new_blockers.join(","))
                    };
                    self.db.execute(
                        "UPDATE tasks SET blocked_by = ?1 WHERE id = ?2",
                        params![new_blocked_by, t.id],
                    )?;
                }
            }
        }

        // Delete all tasks
        for tid in &all_to_delete {
            self.db.execute("DELETE FROM tasks WHERE id = ?1", params![tid])?;
        }

        let detail = if all_to_delete.len() > 1 {
            format!("{} (and {} children)", task_id, all_to_delete.len() - 1)
        } else {
            task_id.to_string()
        };
        self.log_history("task_delete", Some(&detail))?;

        Ok(())
    }

    /// Get claimed task for a worker
    pub fn get_claimed_task(&self, worker_name: &str) -> StateResult<Option<Task>> {
        let result = self.db.query_row(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks WHERE claimed_by = ?1 AND status = ?2",
            params![worker_name, TaskStatus::Doing.as_str()],
            Self::task_from_row,
        );
        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set task tokens used
    pub fn set_task_tokens(&self, task_id: &str, tokens: i64) -> StateResult<()> {
        self.db.execute(
            "UPDATE tasks SET tokens_used = ?1 WHERE id = ?2",
            params![tokens, task_id],
        )?;
        Ok(())
    }

    /// Get average task tokens
    pub fn get_avg_task_tokens(&self) -> StateResult<i64> {
        match self.db.query_row(
            "SELECT AVG(tokens_used) FROM tasks WHERE status = 'done' AND tokens_used IS NOT NULL AND id != 'scope'",
            [],
            |row| row.get::<_, Option<f64>>(0),
        ) {
            Ok(Some(val)) => Ok(val as i64),
            Ok(None) => Ok(0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Admin complete a task (bypasses claim check)
    pub fn admin_complete_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Err(StateError::NotFound(format!("Task '{}' not found", task_id))),
        };

        if task.status == TaskStatus::Done {
            return Ok(()); // Already done
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![TaskStatus::Done.as_str(), self.now(), task_id],
        )?;

        self.log_history("task_done", Some(&format!("{} (admin)", task_id)))?;

        // Auto-complete parent if all siblings are done
        if let Some(parent_id) = &task.parent_id {
            self.maybe_complete_parent(parent_id)?;
        }

        Ok(())
    }

    /// Admin unclaim a task (bypasses worker check)
    pub fn admin_unclaim_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Err(StateError::NotFound(format!("Task '{}' not found", task_id))),
        };

        if task.status != TaskStatus::Doing {
            return Ok(()); // Not claimed
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, claimed_by = NULL, claimed_at = NULL WHERE id = ?2",
            params![TaskStatus::Todo.as_str(), task_id],
        )?;

        self.log_history(
            "task_unclaim",
            Some(&format!("{} (admin)", task_id)),
        )?;
        Ok(())
    }

    // =========================================================================
    // Worker Methods
    // =========================================================================

    fn worker_from_row(row: &Row) -> rusqlite::Result<Worker> {
        Ok(Worker {
            id: row.get("id")?,
            name: row.get("name")?,
            pid: row.get("pid")?,
            session_id: row.get("session_id")?,
            session_started_at: row.get("session_started_at")?,
            status: WorkerStatus::from_str(&row.get::<_, String>("status")?).unwrap_or(WorkerStatus::Idle),
            work_dir: row.get("work_dir")?,
            waiting_thread: row.get("waiting_thread")?,
            needs_restart: row.get::<_, Option<i64>>("needs_restart")?.map(|v| v != 0).unwrap_or(false),
            location: row.get::<_, Option<String>>("location")?.unwrap_or_else(|| "local".to_string()),
            last_heartbeat: row.get("last_heartbeat")?,
            created_at: row.get("created_at")?,
        })
    }

    /// Add a new worker
    pub fn add_worker(&self, name: &str, work_dir: &str, location: &str) -> StateResult<Option<Worker>> {
        match self.db.execute(
            "INSERT INTO workers (name, status, work_dir, location, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, WorkerStatus::Idle.as_str(), work_dir, location, self.now()],
        ) {
            Ok(_) => {
                self.log_history("worker_add", Some(name))?;
                self.get_worker(name)
            }
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.extended_code == 1555 => {
                Ok(None) // Already exists
            }
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get a worker by name
    pub fn get_worker(&self, name: &str) -> StateResult<Option<Worker>> {
        let result = self.db.query_row(
            "SELECT id, name, pid, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at FROM workers WHERE name = ?1",
            params![name],
            Self::worker_from_row,
        );
        match result {
            Ok(worker) => Ok(Some(worker)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get all workers
    pub fn get_workers(&self) -> StateResult<Vec<Worker>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, pid, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at FROM workers ORDER BY id"
        )?;
        let workers = stmt.query_map([], Self::worker_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(workers)
    }

    /// Get active workers (not awaiting or error)
    pub fn get_active_workers(&self) -> StateResult<Vec<Worker>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, pid, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at FROM workers WHERE status NOT IN (?1, ?2) ORDER BY id"
        )?;
        let workers = stmt.query_map(
            params![WorkerStatus::Awaiting.as_str(), WorkerStatus::Error.as_str()],
            Self::worker_from_row
        )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(workers)
    }

    /// Update worker fields
    pub fn update_worker(&self, name: &str, updates: WorkerUpdate) -> StateResult<()> {
        let mut set_clauses = vec![];
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if let Some(pid) = updates.pid {
            set_clauses.push("pid = ?");
            params_vec.push(Box::new(pid));
        }
        if let Some(session_id) = &updates.session_id {
            set_clauses.push("session_id = ?");
            params_vec.push(Box::new(session_id.clone()));
            set_clauses.push("session_started_at = ?");
            params_vec.push(Box::new(self.now()));
        }
        if let Some(status) = updates.status {
            // Check old status for history logging
            let old_status = self.get_worker(name)?.map(|w| w.status);
            set_clauses.push("status = ?");
            params_vec.push(Box::new(status.as_str().to_string()));

            // Log status change if different
            if old_status != Some(status) {
                self.log_history("worker_status", Some(&format!("{} → {}", name, status)))?;
            }
        }
        if let Some(waiting_thread) = &updates.waiting_thread {
            set_clauses.push("waiting_thread = ?");
            params_vec.push(Box::new(waiting_thread.clone()));
        }
        if let Some(needs_restart) = updates.needs_restart {
            set_clauses.push("needs_restart = ?");
            params_vec.push(Box::new(if needs_restart { 1i64 } else { 0i64 }));
        }
        if let Some(last_heartbeat) = &updates.last_heartbeat {
            set_clauses.push("last_heartbeat = ?");
            params_vec.push(Box::new(last_heartbeat.clone()));
        }

        if set_clauses.is_empty() {
            return Ok(());
        }

        params_vec.push(Box::new(name.to_string()));
        let sql = format!(
            "UPDATE workers SET {} WHERE name = ?",
            set_clauses.join(", ")
        );

        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        self.db.execute(&sql, params_refs.as_slice())?;
        Ok(())
    }

    /// Check if all workers are inactive (awaiting or error)
    pub fn all_workers_inactive(&self) -> StateResult<bool> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM workers WHERE status NOT IN (?1, ?2)",
            params![WorkerStatus::Awaiting.as_str(), WorkerStatus::Error.as_str()],
            |row| row.get(0),
        )?;
        Ok(count == 0)
    }

    /// Pause all workers
    pub fn pause_all_workers(&self, reason: &str) -> StateResult<()> {
        self.set_status(Status::Paused)?;
        self.set_waiting_reason(Some(reason))?;

        let workers = self.get_workers()?;
        for w in workers {
            if w.status == WorkerStatus::Working {
                self.update_worker(&w.name, WorkerUpdate {
                    status: Some(WorkerStatus::Paused),
                    ..Default::default()
                })?;
            }
        }

        let detail = if reason.len() > 100 { &reason[..100] } else { reason };
        self.log_history("pause_all", Some(detail))?;
        Ok(())
    }

    /// Resume all workers
    pub fn resume_all_workers(&self) -> StateResult<()> {
        self.set_status(Status::Working)?;
        self.set_waiting_reason(None)?;

        let workers = self.get_workers()?;
        for w in workers {
            if w.status == WorkerStatus::Paused {
                self.update_worker(&w.name, WorkerUpdate {
                    status: Some(WorkerStatus::Working),
                    ..Default::default()
                })?;
            }
        }

        self.log_history("resume_all", None)?;
        Ok(())
    }

    /// Get waiting count
    pub fn get_waiting_count(&self) -> StateResult<i64> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM workers WHERE status = ?1",
            params![WorkerStatus::Waiting.as_str()],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // =========================================================================
    // Eval Methods
    // =========================================================================

    fn eval_from_row(row: &Row) -> rusqlite::Result<Eval> {
        Ok(Eval {
            id: row.get("id")?,
            branch: row.get("branch")?,
            eval_name: row.get("eval_name")?,
            status: EvalStatus::from_str(&row.get::<_, String>("status")?).unwrap_or(EvalStatus::Running),
            feedback: row.get("feedback")?,
            log_file: row.get("log_file")?,
            started_at: row.get("started_at")?,
            finished_at: row.get("finished_at")?,
        })
    }

    /// Start a new eval
    pub fn start_eval(&self, branch: &str, eval_name: Option<&str>, log_file: Option<&str>) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO evals (branch, status, started_at, eval_name, log_file) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![branch, EvalStatus::Running.as_str(), self.now(), eval_name, log_file],
        )?;
        let id = self.db.last_insert_rowid();
        self.log_history("eval_start", Some(&format!("branch={} name={:?}", branch, eval_name)))?;
        Ok(id)
    }

    /// Complete an eval
    pub fn complete_eval(&self, eval_id: i64, success: bool, feedback: &str) -> StateResult<()> {
        let status = if success { EvalStatus::Passed } else { EvalStatus::Failed };
        self.db.execute(
            "UPDATE evals SET status = ?1, feedback = ?2, finished_at = ?3 WHERE id = ?4",
            params![status.as_str(), feedback, self.now(), eval_id],
        )?;
        self.log_history("eval_complete", Some(&format!("id={} status={}", eval_id, status)))?;
        Ok(())
    }

    /// Get an eval by ID
    pub fn get_eval(&self, eval_id: i64) -> StateResult<Option<Eval>> {
        let result = self.db.query_row(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file FROM evals WHERE id = ?1",
            params![eval_id],
            Self::eval_from_row,
        );
        match result {
            Ok(eval) => Ok(Some(eval)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get eval by name
    pub fn get_eval_by_name(&self, eval_name: &str) -> StateResult<Option<Eval>> {
        let result = self.db.query_row(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file FROM evals WHERE eval_name = ?1 ORDER BY id DESC LIMIT 1",
            params![eval_name],
            Self::eval_from_row,
        );
        match result {
            Ok(eval) => Ok(Some(eval)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get all evals
    pub fn get_evals(&self, limit: i64) -> StateResult<Vec<Eval>> {
        let mut stmt = self.db.prepare(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file FROM evals ORDER BY id ASC LIMIT ?1"
        )?;
        let evals = stmt.query_map(params![limit], Self::eval_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(evals)
    }

    /// Get running eval
    pub fn get_running_eval(&self) -> StateResult<Option<Eval>> {
        let result = self.db.query_row(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file FROM evals WHERE status = ?1 ORDER BY id DESC LIMIT 1",
            params![EvalStatus::Running.as_str()],
            Self::eval_from_row,
        );
        match result {
            Ok(eval) => Ok(Some(eval)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Cancel running evals
    pub fn cancel_running_evals(&self, reason: &str) -> StateResult<i64> {
        let mut stmt = self.db.prepare(
            "SELECT id, eval_name FROM evals WHERE status = ?1"
        )?;
        let running: Vec<(i64, Option<String>)> = stmt.query_map(
            params![EvalStatus::Running.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?.collect::<Result<Vec<_>, _>>()?;

        for (id, name) in &running {
            self.db.execute(
                "UPDATE evals SET status = ?1, feedback = ?2, finished_at = ?3 WHERE id = ?4",
                params![EvalStatus::Failed.as_str(), reason, self.now(), id],
            )?;
            self.log_history("eval_cancel", Some(&format!("id={} name={:?}", id, name)))?;
        }

        Ok(running.len() as i64)
    }

    // =========================================================================
    // Message Methods
    // =========================================================================

    fn message_from_row(row: &Row) -> rusqlite::Result<Message> {
        Ok(Message {
            id: row.get("id")?,
            thread: row.get("thread")?,
            sender: row.get("sender")?,
            content: row.get("content")?,
            timestamp: row.get("timestamp")?,
            waiting: row.get::<_, i64>("waiting")? != 0,
        })
    }

    /// Add a message
    pub fn add_message(&self, thread: &str, sender: &str, content: &str, waiting: bool) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO messages (thread, sender, content, timestamp, waiting) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![thread, sender, content, self.now(), if waiting { 1 } else { 0 }],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get messages from a thread
    pub fn get_messages(&self, thread: &str, limit: i64) -> StateResult<Vec<Message>> {
        let mut stmt = self.db.prepare(
            "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE thread = ?1 ORDER BY id LIMIT ?2"
        )?;
        let messages = stmt.query_map(params![thread, limit], Self::message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// Get messages since a timestamp
    pub fn get_messages_since(&self, since: &str, exclude_sender: Option<&str>) -> StateResult<Vec<Message>> {
        if let Some(sender) = exclude_sender {
            let mut stmt = self.db.prepare(
                "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE timestamp > ?1 AND sender != ?2 ORDER BY id"
            )?;
            let messages = stmt.query_map(params![since, sender], Self::message_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        } else {
            let mut stmt = self.db.prepare(
                "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE timestamp > ?1 ORDER BY id"
            )?;
            let messages = stmt.query_map(params![since], Self::message_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        }
    }

    /// Get unread messages for a reader in a thread
    pub fn get_unread_messages(&self, thread: &str, reader: &str) -> StateResult<Vec<Message>> {
        let last_read_id: i64 = self.db.query_row(
            "SELECT last_read_id FROM message_reads WHERE worker_name = ?1 AND thread = ?2",
            params![reader, thread],
            |row| row.get(0),
        ).unwrap_or(0);

        let mut stmt = self.db.prepare(
            "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE thread = ?1 AND id > ?2 AND sender != ?3 ORDER BY id"
        )?;
        let messages = stmt.query_map(params![thread, last_read_id, reader], Self::message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// Get all unread messages for a reader
    pub fn get_all_unread_messages(&self, reader: &str) -> StateResult<Vec<Message>> {
        let threads = self.get_threads()?;
        let mut all_unread = vec![];
        for thread in threads {
            all_unread.extend(self.get_unread_messages(&thread, reader)?);
        }
        Ok(all_unread)
    }

    /// Mark messages as read
    pub fn mark_messages_read(&self, thread: &str, reader: &str, up_to_id: Option<i64>) -> StateResult<()> {
        let max_id = match up_to_id {
            Some(id) => id,
            None => self.db.query_row(
                "SELECT MAX(id) FROM messages WHERE thread = ?1",
                params![thread],
                |row| row.get::<_, Option<i64>>(0),
            )?.unwrap_or(0),
        };

        self.db.execute(
            "INSERT INTO message_reads (worker_name, thread, last_read_id) VALUES (?1, ?2, ?3) ON CONFLICT(worker_name, thread) DO UPDATE SET last_read_id = ?3",
            params![reader, thread, max_id],
        )?;
        Ok(())
    }

    /// Get all thread names
    pub fn get_threads(&self) -> StateResult<Vec<String>> {
        let mut stmt = self.db.prepare("SELECT DISTINCT thread FROM messages ORDER BY thread")?;
        let threads: Vec<String> = stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(threads)
    }

    /// Get message count for a thread
    pub fn get_thread_message_count(&self, thread: &str) -> StateResult<i64> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM messages WHERE thread = ?1",
            params![thread],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // =========================================================================
    // Notification Methods
    // =========================================================================

    /// Increment unread count
    pub fn increment_unread(&self) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET unread_count = COALESCE(unread_count, 0) + 1 WHERE id = 1",
            [],
        )?;
        Ok(())
    }

    /// Clear unread count
    pub fn clear_unread(&self) -> StateResult<()> {
        self.db.execute("UPDATE state SET unread_count = 0 WHERE id = 1", [])?;
        Ok(())
    }

    /// Get unread count
    pub fn get_unread_count(&self) -> StateResult<i64> {
        match self.db.query_row(
            "SELECT unread_count FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(Some(val)) => Ok(val),
            Ok(None) => Ok(0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    // =========================================================================
    // History Methods
    // =========================================================================

    /// Get history entries
    pub fn get_history(&self, limit: i64) -> StateResult<Vec<HistoryEntry>> {
        let mut stmt = self.db.prepare(
            "SELECT id, timestamp, action, detail FROM history ORDER BY id DESC LIMIT ?1"
        )?;
        let entries: Vec<HistoryEntry> = stmt.query_map(params![limit], |row| {
            Ok(HistoryEntry {
                id: row.get("id")?,
                timestamp: row.get("timestamp")?,
                action: row.get("action")?,
                detail: row.get("detail")?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    // =========================================================================
    // Amendment Methods
    // =========================================================================

    /// Add an amendment
    pub fn add_amendment(&self, message: &str, spec_hash: &str, author: &str) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO amendments (message, timestamp, author, spec_hash) VALUES (?1, ?2, ?3, ?4)",
            params![message, self.now(), author, spec_hash],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get all amendments
    pub fn get_amendments(&self) -> StateResult<Vec<Amendment>> {
        let mut stmt = self.db.prepare(
            "SELECT id, message, timestamp, author, spec_hash FROM amendments ORDER BY id"
        )?;
        let amendments: Vec<Amendment> = stmt.query_map([], |row| {
            Ok(Amendment {
                id: row.get("id")?,
                message: row.get("message")?,
                timestamp: row.get("timestamp")?,
                author: row.get("author")?,
                spec_hash: row.get("spec_hash")?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(amendments)
    }

    /// Get last spec hash
    pub fn get_last_spec_hash(&self) -> StateResult<Option<String>> {
        let result: Option<String> = self.db.query_row(
            "SELECT spec_hash FROM amendments ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        ).ok();
        Ok(result)
    }

    // =========================================================================
    // Compaction Methods
    // =========================================================================

    /// Compact messages in a thread
    pub fn compact_messages(&self, thread: &str, ids_to_delete: &[i64], summary_content: &str) -> StateResult<()> {
        if ids_to_delete.is_empty() {
            return Ok(());
        }

        // Delete old messages
        let placeholders: String = ids_to_delete.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM messages WHERE id IN ({})", placeholders);
        let params: Vec<&dyn rusqlite::ToSql> = ids_to_delete.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        self.db.execute(&sql, params.as_slice())?;

        // Insert summary message
        self.db.execute(
            "INSERT INTO messages (thread, sender, content, timestamp, waiting) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![thread, "hirsel", summary_content, self.now(), 0],
        )?;

        // Reset last_read_id for all workers on this thread
        self.db.execute("DELETE FROM message_reads WHERE thread = ?1", params![thread])?;

        self.log_history(
            "compaction",
            Some(&format!("Compacted {} messages in thread '{}'", ids_to_delete.len(), thread)),
        )?;

        Ok(())
    }

    // =========================================================================
    // Worker Event Methods
    // =========================================================================

    /// Insert a text output event
    pub fn insert_text_event(&self, worker_name: &str, content: &str) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, content)
             VALUES (?1, ?2, ?3, ?4)",
            params![worker_name, "text", self.now(), content],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Insert a thought event
    pub fn insert_thought_event(&self, worker_name: &str, content: &str) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, content)
             VALUES (?1, ?2, ?3, ?4)",
            params![worker_name, "thought", self.now(), content],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Insert a tool start event
    pub fn insert_tool_start_event(
        &self,
        worker_name: &str,
        tool_call_id: &str,
        title: &str,
        kind: Option<&str>,
        status: ToolCallStatus,
        input: Option<&str>,
    ) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, tool_call_id, tool_title, tool_kind, tool_status, tool_input)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                worker_name,
                "tool_start",
                self.now(),
                tool_call_id,
                title,
                kind,
                status.as_str(),
                input
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Insert a tool update event
    pub fn insert_tool_update_event(
        &self,
        worker_name: &str,
        tool_call_id: &str,
        title: Option<&str>,
        status: Option<ToolCallStatus>,
        output: Option<&str>,
    ) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, tool_call_id, tool_title, tool_status, tool_output)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                worker_name,
                "tool_update",
                self.now(),
                tool_call_id,
                title,
                status.map(|s| s.as_str()),
                output
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get worker events since a given ID (for polling)
    pub fn get_worker_events(&self, worker_name: &str, after_id: Option<i64>, limit: i64) -> StateResult<Vec<WorkerEvent>> {
        let after = after_id.unwrap_or(0);
        let mut stmt = self.db.prepare(
            "SELECT id, worker_name, event_type, timestamp, content,
                    tool_call_id, tool_title, tool_kind, tool_status, tool_input, tool_output
             FROM worker_events
             WHERE worker_name = ?1 AND id > ?2
             ORDER BY id ASC
             LIMIT ?3"
        )?;

        let events: Vec<WorkerEvent> = stmt.query_map(params![worker_name, after, limit], |row| {
            let event_type_str: String = row.get("event_type")?;
            let tool_status_str: Option<String> = row.get("tool_status")?;

            Ok(WorkerEvent {
                id: row.get("id")?,
                worker_name: row.get("worker_name")?,
                event_type: WorkerEventType::from_str(&event_type_str).unwrap_or(WorkerEventType::Text),
                timestamp: row.get("timestamp")?,
                content: row.get("content")?,
                tool_call_id: row.get("tool_call_id")?,
                tool_title: row.get("tool_title")?,
                tool_kind: row.get("tool_kind")?,
                tool_status: tool_status_str.and_then(|s| ToolCallStatus::from_str(&s)),
                tool_input: row.get("tool_input")?,
                tool_output: row.get("tool_output")?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    /// Get all events for a worker (for initial load)
    pub fn get_all_worker_events(&self, worker_name: &str, limit: i64) -> StateResult<Vec<WorkerEvent>> {
        self.get_worker_events(worker_name, None, limit)
    }

    /// Clear old worker events (for cleanup)
    pub fn clear_worker_events(&self, worker_name: &str) -> StateResult<()> {
        self.db.execute(
            "DELETE FROM worker_events WHERE worker_name = ?1",
            params![worker_name],
        )?;
        Ok(())
    }
}

// =============================================================================
// Worker Update Helper
// =============================================================================

#[derive(Default)]
pub struct WorkerUpdate {
    pub pid: Option<i64>,
    pub session_id: Option<String>,
    pub status: Option<WorkerStatus>,
    pub waiting_thread: Option<String>,
    pub needs_restart: Option<bool>,
    pub last_heartbeat: Option<String>,
}

// =============================================================================
// Type Alias for backwards compatibility
// =============================================================================

pub type State = SQLiteState;
