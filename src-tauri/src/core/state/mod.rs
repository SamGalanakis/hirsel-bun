//! SQLite state management for Hirsel runs
//!
//! This module provides the core state management functionality for tracking
//! runs, tasks, workers, evals, and messages.
//!
//! All methods are async using sqlx for true non-blocking database access.

#![allow(clippy::should_implement_trait)]

mod evals;
mod events;
mod history;
mod run;
mod scribe;
pub mod types;
mod workers;

use sqlx::sqlite::SqlitePool;

use super::db::utc_now;

pub use scribe::ScribeSubmission;
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
    human_in_the_loop INTEGER DEFAULT 1,
    summary TEXT,
    waiting_reason TEXT,
    worker_scale TEXT,
    time_limit_minutes INTEGER,
    started_at TEXT,
    last_time_notification_pct INTEGER,
    last_compaction_at TEXT,
    iteration_count INTEGER DEFAULT 0,
    max_iterations INTEGER,
    pause_mode TEXT DEFAULT 'sender',
    is_test INTEGER DEFAULT 0,
    failure_reason TEXT,
    default_runner TEXT,
    worker_runners TEXT,
    starting_point TEXT,
    runner_configs TEXT,
    scribe_batch_started_at TEXT,
    docs_version INTEGER DEFAULT 0,
    docs_path TEXT,
    persist_docs_changes INTEGER DEFAULT 1,
    project_id INTEGER,
    project_name TEXT,
    route_id INTEGER,

    -- Dispatch tracking
    source_task_ids TEXT,      -- JSON array of task IDs from board
    board_snapshot TEXT,       -- JSON snapshot of board state at dispatch
    branch_off_commit TEXT,    -- Target branch commit SHA at dispatch

    -- Delivery tracking
    delivery_status TEXT DEFAULT 'pending',  -- pending/pushed/pr_open/merged/abandoned
    delivery_branch TEXT,      -- e.g., "hirsel/run-name"
    pr_url TEXT,               -- GitHub/GitLab PR URL
    pr_number INTEGER,
    merged_at TEXT,
    abandoned_at TEXT,

    -- Merge state
    staleness_commits INTEGER DEFAULT 0,  -- Commits on target since branch-off
    merge_state TEXT DEFAULT 'unknown',   -- unknown/clean/conflicts

    -- Scaling check flag (event-driven worker spawning)
    scaling_check_requested INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS workers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    pid INTEGER,
    runner_id TEXT,
    runner_type TEXT,
    session_id TEXT,
    session_started_at TEXT,
    status TEXT NOT NULL DEFAULT 'working',
    work_dir TEXT,
    waiting_thread TEXT,
    needs_restart INTEGER DEFAULT 0,
    location TEXT DEFAULT 'local',
    last_heartbeat TEXT,
    created_at TEXT NOT NULL,
    hitl_waiting INTEGER DEFAULT 0,
    state_handle TEXT,
    -- Direct task assignment fields
    assigned_task_id TEXT,    -- Currently assigned task
    last_task_id TEXT         -- Last completed task (for tree distance)
);

CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    action TEXT NOT NULL,
    detail TEXT
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

-- Scribe submissions for documentation updates
-- status: 'pending', 'processing', 'done', 'failed'
CREATE TABLE IF NOT EXISTS scribe_submissions (
    id INTEGER PRIMARY KEY,
    worker_name TEXT NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    batch_id INTEGER,
    retry_count INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    processed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_scribe_status ON scribe_submissions(status);
CREATE INDEX IF NOT EXISTS idx_worker_events_worker ON worker_events(worker_name);
CREATE INDEX IF NOT EXISTS idx_worker_events_timestamp ON worker_events(timestamp);
"#;

// =============================================================================
// SQLiteState Implementation
// =============================================================================

/// SQLite-backed state management for a hirsel run
pub struct SQLiteState {
    run_name: String,
}

impl SQLiteState {
    /// Create a new SQLiteState for the given run, initializing the database
    pub async fn new(run_name: &str) -> StateResult<Self> {
        let pool = crate::core::db::run_pool(run_name).await;

        // Initialize schema
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;

        Ok(Self {
            run_name: run_name.to_string(),
        })
    }

    /// Get the run name
    pub fn run_name(&self) -> &str {
        &self.run_name
    }

    /// Get the database pool for this run
    pub(crate) async fn pool(&self) -> SqlitePool {
        crate::core::db::run_pool(&self.run_name).await
    }

    pub(crate) async fn log_history(&self, action: &str, detail: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("INSERT INTO history (timestamp, action, detail) VALUES (?, ?, ?)")
            .bind(utc_now())
            .bind(action)
            .bind(detail)
            .execute(&pool)
            .await?;
        Ok(())
    }
}
