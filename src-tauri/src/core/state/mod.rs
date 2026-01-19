//! SQLite state management for Hirsel runs
//!
//! This module provides the core state management functionality for tracking
//! runs, tasks, workers, evals, and messages.

#![allow(clippy::should_implement_trait)]

mod evals;
mod events;
mod history;
mod messages;
mod run;
mod tasks;
pub mod types;
mod workers;

use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::PathBuf;

pub use types::*;

// =============================================================================
// Schema
// =============================================================================

const SCHEMA: &str = r#"
-- hirsel run state schema

CREATE TABLE IF NOT EXISTS state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    status TEXT NOT NULL DEFAULT 'draft',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    request TEXT,
    project_path TEXT,
    remote_url TEXT,
    branch TEXT,
    unread_count INTEGER DEFAULT 0,
    human_in_the_loop INTEGER DEFAULT 1,
    summary TEXT,
    waiting_reason TEXT,
    worker_scale TEXT,
    time_limit_minutes INTEGER,
    started_at TEXT,
    last_time_notification_pct INTEGER,
    learnings_processed_at TEXT,
    last_compaction_at TEXT,
    iteration_count INTEGER DEFAULT 0,
    max_iterations INTEGER,
    pause_mode TEXT DEFAULT 'sender',
    is_test INTEGER DEFAULT 0,
    failure_reason TEXT,
    default_runner TEXT,
    worker_runners TEXT
);

CREATE TABLE IF NOT EXISTS workers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    pid INTEGER,
    session_id TEXT,
    session_started_at TEXT,
    status TEXT NOT NULL DEFAULT 'working',
    work_dir TEXT,
    waiting_thread TEXT,
    needs_restart INTEGER DEFAULT 0,
    location TEXT DEFAULT 'local',
    last_heartbeat TEXT,
    created_at TEXT NOT NULL,
    hitl_waiting INTEGER DEFAULT 0
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
    pid INTEGER,
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
        self.run_migrations()?;
        Ok(())
    }

    /// Run database migrations for schema changes
    /// Note: In development mode, we don't need migrations - just delete ~/.hirsel/runs
    fn run_migrations(&mut self) -> StateResult<()> {
        Ok(())
    }

    pub(crate) fn now(&self) -> String {
        Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
    }

    pub(crate) fn log_history(&self, action: &str, detail: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "INSERT INTO history (timestamp, action, detail) VALUES (?1, ?2, ?3)",
            params![self.now(), action, detail],
        )?;
        Ok(())
    }
}
