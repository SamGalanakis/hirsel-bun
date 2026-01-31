//! Run state methods
//!
//! Methods for managing run-level state: status, request, project info, time tracking, etc.

use chrono::{DateTime, Local};
use rusqlite::params;

use super::types::{FailureReason, StateError, StateResult, Status, TimeInfo};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // Run State Methods
    // =========================================================================

    /// Get the current run status
    pub fn status(&self) -> StateResult<Status> {
        let status: String = self
            .db
            .query_row("SELECT status FROM state WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|_| "draft".to_string());

        Ok(Status::from_str(&status).unwrap_or(Status::Draft))
    }

    /// Check if a status transition is valid
    fn can_transition_to(&self, from: Status, to: Status) -> bool {
        use Status::*;
        matches!(
            (from, to),
            // From Draft
            (Draft, Working) |
            // From Working
            (Working, Paused)
            | (Working, Failed)
            | (Working, Eval)
            | (Working, Done) | // No eval configured
            // From Paused
            (Paused, Working)
            | (Paused, Failed) |
            // From Failed (allow resume/retry)
            (Failed, Working)
            | (Failed, Paused) |
            // From Eval
            (Eval, Done)
            | (Eval, Failed)
            | (Eval, Working)
            | (Eval, Paused) | // Pausing during eval
            // From Done
            (Done, Delivered) |
            // Setting same status is always allowed
            (_, _) if from == to
        )
    }

    /// Set the run status with optional transition validation
    ///
    /// Set `validate` to true to enforce valid state transitions.
    /// When transitioning away from Failed, the failure_reason is cleared.
    pub fn set_status_validated(&self, status: Status, validate: bool) -> StateResult<()> {
        let old_status = self.status()?;
        if old_status == status {
            return Ok(()); // No change
        }

        // Validate transition if requested
        if validate && !self.can_transition_to(old_status, status) {
            return Err(StateError::InvalidTransition(old_status, status));
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

        // Clear failure_reason when transitioning away from Failed
        if old_status == Status::Failed && status != Status::Failed {
            self.set_failure_reason(None)?;
        }

        self.log_history("status_change", Some(&format!("run {}", status)))?;
        Ok(())
    }

    /// Set the run status (no validation for backward compatibility)
    pub fn set_status(&self, status: Status) -> StateResult<()> {
        self.set_status_validated(status, false)
    }

    /// Initialize the state for a new run
    pub fn init_state(&self, project_path: Option<&str>) -> StateResult<()> {
        let now = self.now();
        self.db.execute(
            r#"
            INSERT OR REPLACE INTO state (id, status, created_at, updated_at, project_path)
            VALUES (1, ?1, ?2, ?2, ?3)
            "#,
            params![Status::Draft.as_str(), now, project_path],
        )?;
        self.log_history("init", None)?;
        Ok(())
    }

    /// Get the request text
    pub fn get_request(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT request FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
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
        match self
            .db
            .query_row("SELECT waiting_reason FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
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
        match self
            .db
            .query_row("SELECT project_path FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
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

    /// Clear project path (set to NULL)
    pub fn clear_project_path(&self) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET project_path = NULL, updated_at = ?1 WHERE id = 1",
            params![self.now()],
        )?;
        Ok(())
    }

    /// Get remote URL (for remote git repos)
    pub fn get_remote_url(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT remote_url FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set remote URL (for remote git repos)
    pub fn set_remote_url(&self, url: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET remote_url = ?1, updated_at = ?2 WHERE id = 1",
            params![url, self.now()],
        )?;
        Ok(())
    }

    /// Get branch (source branch for the run)
    pub fn get_branch(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT branch FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set branch (source branch for the run)
    pub fn set_branch(&self, branch: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET branch = ?1, updated_at = ?2 WHERE id = 1",
            params![branch, self.now()],
        )?;
        Ok(())
    }

    /// Get project ID
    pub fn get_project_id(&self) -> StateResult<Option<i64>> {
        match self
            .db
            .query_row("SELECT project_id FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<i64>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set project ID
    pub fn set_project_id(&self, project_id: i64) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET project_id = ?1, updated_at = ?2 WHERE id = 1",
            params![project_id, self.now()],
        )?;
        Ok(())
    }

    /// Get project name
    pub fn get_project_name(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT project_name FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set project name
    pub fn set_project_name(&self, project_name: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET project_name = ?1, updated_at = ?2 WHERE id = 1",
            params![project_name, self.now()],
        )?;
        Ok(())
    }

    /// Get created_at timestamp
    pub fn get_created_at(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT created_at FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get updated_at timestamp
    pub fn get_updated_at(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT updated_at FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get summary
    pub fn get_summary(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT summary FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
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
        match self
            .db
            .query_row("SELECT worker_scale FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
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
        // Check if value is actually changing
        let current = self.get_human_in_the_loop()?;
        if current == enabled {
            return Ok(()); // No change, skip logging and notification
        }

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
        match self
            .db
            .query_row("SELECT started_at FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set started_at timestamp
    pub fn set_started_at(&self, timestamp: Option<&str>) -> StateResult<()> {
        let ts = timestamp
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.now());
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
        let row: Option<(Option<i64>, Option<String>)> = self
            .db
            .query_row(
                "SELECT time_limit_minutes, started_at FROM state WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        match row {
            Some((Some(limit_minutes), Some(started_at_str))) => {
                let started_at = DateTime::parse_from_rfc3339(&started_at_str)
                    .or_else(|_| DateTime::parse_from_str(&started_at_str, "%Y-%m-%dT%H:%M:%S%.f"))
                    .map(|dt| dt.with_timezone(&Local))
                    .map_err(|_| {
                        StateError::InvalidState("Invalid started_at timestamp".to_string())
                    })?;

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

    // =========================================================================
    // Failure Reason
    // =========================================================================

    /// Get the failure reason (only meaningful when status is Failed)
    pub fn get_failure_reason(&self) -> StateResult<Option<FailureReason>> {
        match self
            .db
            .query_row("SELECT failure_reason FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(Some(val)) => Ok(FailureReason::from_str(&val)),
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the failure reason
    pub fn set_failure_reason(&self, reason: Option<FailureReason>) -> StateResult<()> {
        let reason_str = reason.map(|r| r.as_str().to_string());
        self.db.execute(
            "UPDATE state SET failure_reason = ?1, updated_at = ?2 WHERE id = 1",
            params![reason_str, self.now()],
        )?;
        Ok(())
    }

    /// Set status to Failed with a reason
    pub fn set_failed(&self, reason: FailureReason) -> StateResult<()> {
        self.set_failure_reason(Some(reason))?;
        self.set_status(Status::Failed)?;
        Ok(())
    }

    /// Get pause mode ("sender" or "all")
    pub fn get_pause_mode(&self) -> StateResult<String> {
        match self
            .db
            .query_row("SELECT pause_mode FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
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

    /// Check if this is a test run (auto-cleanup after eval)
    pub fn is_test_run(&self) -> StateResult<bool> {
        let result: i64 = self.db.query_row(
            "SELECT COALESCE(is_test, 0) FROM state WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(result != 0)
    }

    /// Mark this run as a test run (will auto-cleanup after eval)
    pub fn set_is_test(&self, is_test: bool) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET is_test = ?1, updated_at = ?2 WHERE id = 1",
            params![if is_test { 1 } else { 0 }, self.now()],
        )?;
        Ok(())
    }

    // =========================================================================
    // Runner Configuration
    // =========================================================================

    /// Get the default runner for this run
    pub fn get_default_runner(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT default_runner FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the default runner for this run
    pub fn set_default_runner(&self, runner: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET default_runner = ?1, updated_at = ?2 WHERE id = 1",
            params![runner, self.now()],
        )?;
        Ok(())
    }

    /// Get per-worker runner assignments as JSON
    pub fn get_worker_runners(
        &self,
    ) -> StateResult<Option<std::collections::HashMap<String, String>>> {
        match self
            .db
            .query_row("SELECT worker_runners FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(Some(json)) => {
                let map: std::collections::HashMap<String, String> =
                    serde_json::from_str(&json).unwrap_or_default();
                Ok(Some(map))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set per-worker runner assignments as JSON
    pub fn set_worker_runners(
        &self,
        runners: Option<&std::collections::HashMap<String, String>>,
    ) -> StateResult<()> {
        let json = runners.map(|r| serde_json::to_string(r).unwrap_or_default());
        self.db.execute(
            "UPDATE state SET worker_runners = ?1, updated_at = ?2 WHERE id = 1",
            params![json, self.now()],
        )?;
        Ok(())
    }

    /// Get the runner name for a specific worker (falls back to default_runner, then "local")
    pub fn get_runner_for_worker(&self, worker_name: &str) -> StateResult<String> {
        // First check per-worker assignments
        if let Some(runners) = self.get_worker_runners()? {
            if let Some(runner) = runners.get(worker_name) {
                return Ok(runner.clone());
            }
        }
        // Fall back to default runner
        if let Some(default) = self.get_default_runner()? {
            return Ok(default);
        }
        // Ultimate fallback
        Ok("local".to_string())
    }

    /// Get runner configs stored at run creation time.
    ///
    /// Returns a map of runner name -> RunnerConfig. This captures the full
    /// configuration at the time the run was created, ensuring that config
    /// changes don't affect in-progress runs.
    pub fn get_runner_configs(
        &self,
    ) -> StateResult<Option<std::collections::HashMap<String, crate::core::runner::RunnerConfig>>>
    {
        match self
            .db
            .query_row("SELECT runner_configs FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(Some(json)) => {
                let map: std::collections::HashMap<String, crate::core::runner::RunnerConfig> =
                    serde_json::from_str(&json).unwrap_or_default();
                Ok(Some(map))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set runner configs at run creation time.
    ///
    /// This captures the full configuration for all runners used by this run,
    /// ensuring that config changes don't affect in-progress runs.
    pub fn set_runner_configs(
        &self,
        configs: Option<&std::collections::HashMap<String, crate::core::runner::RunnerConfig>>,
    ) -> StateResult<()> {
        let json = configs.map(|c| serde_json::to_string(c).unwrap_or_default());
        self.db.execute(
            "UPDATE state SET runner_configs = ?1, updated_at = ?2 WHERE id = 1",
            params![json, self.now()],
        )?;
        Ok(())
    }

    /// Get the resolved runner config for a specific worker.
    ///
    /// Looks up the runner config from stored runner_configs (captured at run creation).
    pub fn get_runner_config_for_worker(
        &self,
        worker_name: &str,
    ) -> StateResult<crate::core::runner::RunnerConfig> {
        // Get the runner name for this worker
        let runner_name = self.get_runner_for_worker(worker_name)?;

        // Look up from stored configs
        if let Some(configs) = self.get_runner_configs()? {
            if let Some(config) = configs.get(&runner_name) {
                return Ok(config.clone());
            }
        }

        // No stored config - use local runner
        Ok(crate::core::runner::RunnerConfig::local())
    }

    // =========================================================================
    // Starting Point
    // =========================================================================

    /// Get the starting point as JSON
    pub fn get_starting_point(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT starting_point FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the starting point as JSON
    pub fn set_starting_point(&self, starting_point: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET starting_point = ?1, updated_at = ?2 WHERE id = 1",
            params![starting_point, self.now()],
        )?;
        Ok(())
    }

    // =========================================================================
    // Docs Persistence Settings
    // =========================================================================

    /// Get the docs path for this run (relative to workspace)
    pub fn get_docs_path(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT docs_path FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the docs path for this run (relative to workspace)
    pub fn set_docs_path(&self, path: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET docs_path = ?1, updated_at = ?2 WHERE id = 1",
            params![path, self.now()],
        )?;
        Ok(())
    }

    /// Get whether to persist docs changes on delivery
    pub fn get_persist_docs_changes(&self) -> StateResult<bool> {
        match self.db.query_row(
            "SELECT persist_docs_changes FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(Some(val)) => Ok(val != 0),
            Ok(None) => Ok(true), // Default to persisting
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set whether to persist docs changes on delivery
    pub fn set_persist_docs_changes(&self, persist: bool) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET persist_docs_changes = ?1, updated_at = ?2 WHERE id = 1",
            params![if persist { 1 } else { 0 }, self.now()],
        )?;
        Ok(())
    }

    // =========================================================================
    // Dispatch Tracking
    // =========================================================================

    /// Get the source task IDs (JSON array of task IDs from board dispatch)
    pub fn get_source_task_ids(&self) -> StateResult<Option<Vec<String>>> {
        match self.db.query_row(
            "SELECT source_task_ids FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(Some(json)) => {
                let ids: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
                Ok(Some(ids))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the source task IDs
    pub fn set_source_task_ids(&self, task_ids: Option<&[String]>) -> StateResult<()> {
        let json = task_ids.map(|ids| serde_json::to_string(ids).unwrap_or_default());
        self.db.execute(
            "UPDATE state SET source_task_ids = ?1, updated_at = ?2 WHERE id = 1",
            params![json, self.now()],
        )?;
        Ok(())
    }

    /// Get the board snapshot (JSON snapshot of board state at dispatch)
    pub fn get_board_snapshot(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT board_snapshot FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the board snapshot
    pub fn set_board_snapshot(&self, snapshot: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET board_snapshot = ?1, updated_at = ?2 WHERE id = 1",
            params![snapshot, self.now()],
        )?;
        Ok(())
    }

    /// Get the branch-off commit SHA
    pub fn get_branch_off_commit(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT branch_off_commit FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the branch-off commit SHA
    pub fn set_branch_off_commit(&self, commit: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET branch_off_commit = ?1, updated_at = ?2 WHERE id = 1",
            params![commit, self.now()],
        )?;
        Ok(())
    }

    // =========================================================================
    // Delivery Tracking
    // =========================================================================

    /// Get the delivery status
    pub fn get_delivery_status(&self) -> StateResult<super::types::DeliveryStatus> {
        match self.db.query_row(
            "SELECT delivery_status FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(Some(val)) => Ok(super::types::DeliveryStatus::from_str(&val)
                .unwrap_or(super::types::DeliveryStatus::Pending)),
            Ok(None) => Ok(super::types::DeliveryStatus::Pending),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(super::types::DeliveryStatus::Pending),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the delivery status
    pub fn set_delivery_status(&self, status: super::types::DeliveryStatus) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET delivery_status = ?1, updated_at = ?2 WHERE id = 1",
            params![status.as_str(), self.now()],
        )?;
        Ok(())
    }

    /// Get the delivery branch name
    pub fn get_delivery_branch(&self) -> StateResult<Option<String>> {
        match self.db.query_row(
            "SELECT delivery_branch FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the delivery branch name
    pub fn set_delivery_branch(&self, branch: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET delivery_branch = ?1, updated_at = ?2 WHERE id = 1",
            params![branch, self.now()],
        )?;
        Ok(())
    }

    /// Get the PR URL
    pub fn get_pr_url(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT pr_url FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the PR URL
    pub fn set_pr_url(&self, url: Option<&str>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET pr_url = ?1, updated_at = ?2 WHERE id = 1",
            params![url, self.now()],
        )?;
        Ok(())
    }

    /// Get the PR number
    pub fn get_pr_number(&self) -> StateResult<Option<i64>> {
        match self
            .db
            .query_row("SELECT pr_number FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<i64>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the PR number
    pub fn set_pr_number(&self, number: Option<i64>) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET pr_number = ?1, updated_at = ?2 WHERE id = 1",
            params![number, self.now()],
        )?;
        Ok(())
    }

    /// Get the merged_at timestamp
    pub fn get_merged_at(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT merged_at FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the merged_at timestamp
    pub fn set_merged_at(&self, timestamp: Option<&str>) -> StateResult<()> {
        let ts = timestamp
            .map(|s| s.to_string())
            .or_else(|| Some(self.now()));
        self.db.execute(
            "UPDATE state SET merged_at = ?1, updated_at = ?2 WHERE id = 1",
            params![ts, self.now()],
        )?;
        Ok(())
    }

    /// Get the abandoned_at timestamp
    pub fn get_abandoned_at(&self) -> StateResult<Option<String>> {
        match self
            .db
            .query_row("SELECT abandoned_at FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the abandoned_at timestamp
    pub fn set_abandoned_at(&self, timestamp: Option<&str>) -> StateResult<()> {
        let ts = timestamp
            .map(|s| s.to_string())
            .or_else(|| Some(self.now()));
        self.db.execute(
            "UPDATE state SET abandoned_at = ?1, updated_at = ?2 WHERE id = 1",
            params![ts, self.now()],
        )?;
        Ok(())
    }

    // =========================================================================
    // Merge State
    // =========================================================================

    /// Get the staleness commits count
    pub fn get_staleness_commits(&self) -> StateResult<u32> {
        match self.db.query_row(
            "SELECT staleness_commits FROM state WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(Some(val)) => Ok(val as u32),
            Ok(None) => Ok(0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the staleness commits count
    pub fn set_staleness_commits(&self, count: u32) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET staleness_commits = ?1, updated_at = ?2 WHERE id = 1",
            params![count as i64, self.now()],
        )?;
        Ok(())
    }

    /// Get the merge state
    pub fn get_merge_state(&self) -> StateResult<super::types::MergeState> {
        match self
            .db
            .query_row("SELECT merge_state FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<String>>(0)
            }) {
            Ok(Some(val)) => Ok(super::types::MergeState::from_str(&val)
                .unwrap_or(super::types::MergeState::Unknown)),
            Ok(None) => Ok(super::types::MergeState::Unknown),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(super::types::MergeState::Unknown),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set the merge state
    pub fn set_merge_state(&self, state: super::types::MergeState) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET merge_state = ?1, updated_at = ?2 WHERE id = 1",
            params![state.as_str(), self.now()],
        )?;
        Ok(())
    }

    /// Mark run as delivered (set delivery_status and merged_at)
    pub fn mark_delivered(&self) -> StateResult<()> {
        self.set_delivery_status(super::types::DeliveryStatus::Merged)?;
        self.set_merged_at(None)?;
        self.set_status(Status::Delivered)?;
        self.log_history("delivered", None)?;
        Ok(())
    }

    /// Mark run as abandoned
    pub fn mark_abandoned(&self) -> StateResult<()> {
        self.set_delivery_status(super::types::DeliveryStatus::Abandoned)?;
        self.set_abandoned_at(None)?;
        self.log_history("abandoned", None)?;
        Ok(())
    }

    // =========================================================================
    // Scaling Check (Event-Driven Worker Spawning)
    // =========================================================================

    /// Request a scaling check on the next daemon poll.
    ///
    /// Called by task state changes to signal that worker scaling should be evaluated.
    /// The daemon's 5-second poll provides natural debouncing.
    pub fn request_scaling_check(&self) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET scaling_check_requested = 1 WHERE id = 1",
            [],
        )?;
        Ok(())
    }

    /// Consume the scaling check flag, returning whether it was set.
    ///
    /// Called by the daemon on its polling loop. Atomically reads and clears the flag.
    pub fn consume_scaling_check(&self) -> StateResult<bool> {
        let requested: bool = self
            .db
            .query_row(
                "SELECT scaling_check_requested FROM state WHERE id = 1",
                [],
                |row| row.get::<_, i64>(0).map(|v| v != 0),
            )
            .unwrap_or(false);

        if requested {
            self.db.execute(
                "UPDATE state SET scaling_check_requested = 0 WHERE id = 1",
                [],
            )?;
        }
        Ok(requested)
    }
}
