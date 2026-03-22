//! Run state methods
//!
//! Methods for managing run-level state: status, request, project info, time tracking, etc.

use chrono::{DateTime, Local};
use sqlx::Row;

use super::types::{FailureReason, StateError, StateResult, Status, TimeInfo};
use super::SQLiteState;
use crate::core::db::utc_now;

impl SQLiteState {
    // =========================================================================
    // Run State Methods
    // =========================================================================

    /// Get the current run status
    pub async fn status(&self) -> StateResult<Status> {
        let pool = self.pool().await;
        let status: Option<String> = sqlx::query_scalar("SELECT status FROM state WHERE id = 1")
            .fetch_optional(&pool)
            .await?;

        Ok(
            Status::from_str(&status.unwrap_or_else(|| "draft".to_string()))
                .unwrap_or(Status::Draft),
        )
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
    pub async fn set_status_validated(&self, status: Status, validate: bool) -> StateResult<()> {
        let old_status = self.status().await?;
        if old_status == status {
            return Ok(()); // No change
        }

        // Validate transition if requested
        if validate && !self.can_transition_to(old_status, status) {
            return Err(StateError::InvalidTransition(old_status, status));
        }

        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            r#"
            INSERT INTO state (id, status, created_at, updated_at)
            VALUES (1, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at
            "#,
        )
        .bind(status.as_str())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await?;

        // Clear failure_reason when transitioning away from Failed
        if old_status == Status::Failed && status != Status::Failed {
            self.set_failure_reason(None).await?;
        }

        self.log_history("status_change", Some(&format!("run {}", status)))
            .await?;
        Ok(())
    }

    /// Set the run status with transition validation.
    pub async fn set_status(&self, status: Status) -> StateResult<()> {
        self.set_status_validated(status, true).await
    }

    /// Initialize the state for a new run
    pub async fn init_state(&self, project_path: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO state (id, status, created_at, updated_at, project_path)
            VALUES (1, ?, ?, ?, ?)
            "#,
        )
        .bind(Status::Draft.as_str())
        .bind(&now)
        .bind(&now)
        .bind(project_path)
        .execute(&pool)
        .await?;
        self.log_history("init", None).await?;
        Ok(())
    }

    /// Get the request text
    pub async fn get_request(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT request FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the request text
    pub async fn set_request(&self, request: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET request = ?, updated_at = ? WHERE id = 1")
            .bind(request)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the waiting reason
    pub async fn get_waiting_reason(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT waiting_reason FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the waiting reason
    pub async fn set_waiting_reason(&self, reason: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET waiting_reason = ?, updated_at = ? WHERE id = 1")
            .bind(reason)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get project path
    pub async fn get_project_path(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT project_path FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set project path
    pub async fn set_project_path(&self, path: &str) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET project_path = ?, updated_at = ? WHERE id = 1")
            .bind(path)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Clear project path (set to NULL)
    pub async fn clear_project_path(&self) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET project_path = NULL, updated_at = ? WHERE id = 1")
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get remote URL (for remote git repos)
    pub async fn get_remote_url(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT remote_url FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set remote URL (for remote git repos)
    pub async fn set_remote_url(&self, url: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET remote_url = ?, updated_at = ? WHERE id = 1")
            .bind(url)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get branch (source branch for the run)
    pub async fn get_branch(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT branch FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set branch (source branch for the run)
    pub async fn set_branch(&self, branch: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET branch = ?, updated_at = ? WHERE id = 1")
            .bind(branch)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get project ID
    pub async fn get_project_id(&self) -> StateResult<Option<i64>> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT project_id FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set project ID
    pub async fn set_project_id(&self, project_id: i64) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET project_id = ?, updated_at = ? WHERE id = 1")
            .bind(project_id)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get route ID
    pub async fn get_route_id(&self) -> StateResult<i64> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT route_id FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        result
            .flatten()
            .ok_or_else(|| StateError::NotFound("route_id not set".to_string()))
    }

    /// Set route ID
    pub async fn set_route_id(&self, route_id: i64) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET route_id = ?, updated_at = ? WHERE id = 1")
            .bind(route_id)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get project name
    pub async fn get_project_name(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT project_name FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set project name
    pub async fn set_project_name(&self, project_name: &str) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET project_name = ?, updated_at = ? WHERE id = 1")
            .bind(project_name)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get created_at timestamp
    pub async fn get_created_at(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT created_at FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Get updated_at timestamp
    pub async fn get_updated_at(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT updated_at FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Get summary
    pub async fn get_summary(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT summary FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set summary
    pub async fn set_summary(&self, summary: &str) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET summary = ?, updated_at = ? WHERE id = 1")
            .bind(summary)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        self.log_history("summary_generated", None).await?;
        Ok(())
    }

    /// Get worker scale
    pub async fn get_worker_scale(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT worker_scale FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set worker scale
    pub async fn set_worker_scale(&self, scale: &str) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET worker_scale = ?, updated_at = ? WHERE id = 1")
            .bind(scale)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get human in the loop setting
    pub async fn get_human_in_the_loop(&self) -> StateResult<bool> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT human_in_the_loop FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        match result.flatten() {
            Some(val) => Ok(val != 0),
            None => Ok(true), // Default to HITL
        }
    }

    /// Set human in the loop
    pub async fn set_human_in_the_loop(&self, enabled: bool) -> StateResult<()> {
        // Check if value is actually changing
        let current = self.get_human_in_the_loop().await?;
        if current == enabled {
            return Ok(()); // No change, skip logging and notification
        }

        let pool = self.pool().await;
        sqlx::query("UPDATE state SET human_in_the_loop = ?, updated_at = ? WHERE id = 1")
            .bind(if enabled { 1i64 } else { 0i64 })
            .bind(utc_now())
            .execute(&pool)
            .await?;
        self.log_history("mode_change", Some(if enabled { "hitl" } else { "yolo" }))
            .await?;

        // Notify workers via group chat (project messages)
        if let (Some(project_id), Ok(route_id)) =
            (self.get_project_id().await?, self.get_route_id().await)
        {
            let msg = if enabled {
                "The user is now available. Feel free to message them if needed."
            } else {
                "The user is currently unavailable for messages. Do not attempt to contact them - do the work to the best of your abilities."
            };
            if let Ok(store) = crate::core::ProjectMessagesStore::open().await {
                let _ = store
                    .add_message(project_id, route_id, "chat", "system", msg, false)
                    .await;
            }
        }
        Ok(())
    }

    // =========================================================================
    // Time Tracking Methods
    // =========================================================================

    /// Get time limit in minutes
    pub async fn get_time_limit_minutes(&self) -> StateResult<Option<i64>> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT time_limit_minutes FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set time limit in minutes
    pub async fn set_time_limit_minutes(&self, minutes: Option<i64>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET time_limit_minutes = ?, updated_at = ? WHERE id = 1")
            .bind(minutes)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get started_at timestamp
    pub async fn get_started_at(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT started_at FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set started_at timestamp
    pub async fn set_started_at(&self, timestamp: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        let ts = timestamp.map(|s| s.to_string()).unwrap_or_else(utc_now);
        sqlx::query("UPDATE state SET started_at = ?, updated_at = ? WHERE id = 1")
            .bind(&ts)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Clear time tracking
    pub async fn clear_time_tracking(&self) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET started_at = NULL, last_time_notification_pct = NULL, updated_at = ? WHERE id = 1")
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get last time notification percentage
    pub async fn get_last_time_notification_pct(&self) -> StateResult<Option<i64>> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT last_time_notification_pct FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set last time notification percentage
    pub async fn set_last_time_notification_pct(&self, pct: i64) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET last_time_notification_pct = ?, updated_at = ? WHERE id = 1")
            .bind(pct)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get time info
    pub async fn get_time_info(&self) -> StateResult<Option<TimeInfo>> {
        let pool = self.pool().await;
        let row = sqlx::query("SELECT time_limit_minutes, started_at FROM state WHERE id = 1")
            .fetch_optional(&pool)
            .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let limit_minutes: Option<i64> = row.get("time_limit_minutes");
        let started_at_str: Option<String> = row.get("started_at");

        match (limit_minutes, started_at_str) {
            (Some(limit_minutes), Some(started_at_str)) => {
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
    pub async fn is_time_expired(&self) -> StateResult<bool> {
        match self.get_time_info().await? {
            Some(info) => Ok(info.remaining_minutes <= 0.0),
            None => Ok(false),
        }
    }

    // =========================================================================
    // Iteration Tracking
    // =========================================================================

    /// Get iteration count
    pub async fn get_iteration_count(&self) -> StateResult<i64> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT iteration_count FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten().unwrap_or(0))
    }

    /// Increment iteration count
    pub async fn increment_iteration(&self) -> StateResult<i64> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET iteration_count = COALESCE(iteration_count, 0) + 1, updated_at = ? WHERE id = 1")
            .bind(utc_now())
            .execute(&pool)
            .await?;
        self.get_iteration_count().await
    }

    // =========================================================================
    // Failure Reason
    // =========================================================================

    /// Get the failure reason (only meaningful when status is Failed)
    pub async fn get_failure_reason(&self) -> StateResult<Option<FailureReason>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT failure_reason FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten().and_then(|s| FailureReason::from_str(&s)))
    }

    /// Set the failure reason
    pub async fn set_failure_reason(&self, reason: Option<FailureReason>) -> StateResult<()> {
        let pool = self.pool().await;
        let reason_str = reason.map(|r| r.as_str().to_string());
        sqlx::query("UPDATE state SET failure_reason = ?, updated_at = ? WHERE id = 1")
            .bind(reason_str)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Set status to Failed with a reason
    pub async fn set_failed(&self, reason: FailureReason) -> StateResult<()> {
        self.set_failure_reason(Some(reason)).await?;
        self.set_status(Status::Failed).await?;
        Ok(())
    }

    /// Get pause mode ("sender" or "all")
    pub async fn get_pause_mode(&self) -> StateResult<String> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT pause_mode FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten().unwrap_or_else(|| "sender".to_string()))
    }

    /// Set pause mode ("sender" or "all")
    pub async fn set_pause_mode(&self, mode: &str) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET pause_mode = ?, updated_at = ? WHERE id = 1")
            .bind(mode)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Check if this is a test run (auto-cleanup after eval)
    pub async fn is_test_run(&self) -> StateResult<bool> {
        let pool = self.pool().await;
        let result: i64 = sqlx::query_scalar("SELECT COALESCE(is_test, 0) FROM state WHERE id = 1")
            .fetch_one(&pool)
            .await?;
        Ok(result != 0)
    }

    /// Mark this run as a test run (will auto-cleanup after eval)
    pub async fn set_is_test(&self, is_test: bool) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET is_test = ?, updated_at = ? WHERE id = 1")
            .bind(if is_test { 1i64 } else { 0i64 })
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Runner Configuration
    // =========================================================================

    /// Get the default runner for this run
    pub async fn get_default_runner(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT default_runner FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the default runner for this run
    pub async fn set_default_runner(&self, runner: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET default_runner = ?, updated_at = ? WHERE id = 1")
            .bind(runner)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get per-worker runner assignments as JSON
    pub async fn get_worker_runners(
        &self,
    ) -> StateResult<Option<std::collections::HashMap<String, String>>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT worker_runners FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        match result.flatten() {
            Some(json) => {
                let map: std::collections::HashMap<String, String> =
                    serde_json::from_str(&json).unwrap_or_default();
                Ok(Some(map))
            }
            None => Ok(None),
        }
    }

    /// Set per-worker runner assignments as JSON
    pub async fn set_worker_runners(
        &self,
        runners: Option<&std::collections::HashMap<String, String>>,
    ) -> StateResult<()> {
        let pool = self.pool().await;
        let json = runners.map(|r| serde_json::to_string(r).unwrap_or_default());
        sqlx::query("UPDATE state SET worker_runners = ?, updated_at = ? WHERE id = 1")
            .bind(json)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the runner name for a specific worker (falls back to default_runner, then "local")
    pub async fn get_runner_for_worker(&self, worker_name: &str) -> StateResult<String> {
        // First check per-worker assignments
        if let Some(runners) = self.get_worker_runners().await? {
            if let Some(runner) = runners.get(worker_name) {
                return Ok(runner.clone());
            }
        }
        // Fall back to default runner
        if let Some(default) = self.get_default_runner().await? {
            return Ok(default);
        }
        // Ultimate fallback
        Ok("local".to_string())
    }

    /// Get runner configs stored at run creation time.
    pub async fn get_runner_configs(
        &self,
    ) -> StateResult<Option<std::collections::HashMap<String, crate::core::runner::RunnerConfig>>>
    {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT runner_configs FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        match result.flatten() {
            Some(json) => {
                let map: std::collections::HashMap<String, crate::core::runner::RunnerConfig> =
                    serde_json::from_str(&json).unwrap_or_default();
                Ok(Some(map))
            }
            None => Ok(None),
        }
    }

    /// Set runner configs at run creation time.
    pub async fn set_runner_configs(
        &self,
        configs: Option<&std::collections::HashMap<String, crate::core::runner::RunnerConfig>>,
    ) -> StateResult<()> {
        let pool = self.pool().await;
        let json = configs.map(|c| serde_json::to_string(c).unwrap_or_default());
        sqlx::query("UPDATE state SET runner_configs = ?, updated_at = ? WHERE id = 1")
            .bind(json)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the resolved runner config for a specific worker.
    pub async fn get_runner_config_for_worker(
        &self,
        worker_name: &str,
    ) -> StateResult<crate::core::runner::RunnerConfig> {
        // Get the runner name for this worker
        let runner_name = self.get_runner_for_worker(worker_name).await?;

        // Look up from stored configs
        if let Some(configs) = self.get_runner_configs().await? {
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
    pub async fn get_starting_point(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT starting_point FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the starting point as JSON
    pub async fn set_starting_point(&self, starting_point: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET starting_point = ?, updated_at = ? WHERE id = 1")
            .bind(starting_point)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Dispatch Tracking
    // =========================================================================

    /// Get the source task IDs (JSON array of task IDs from board dispatch)
    pub async fn get_source_task_ids(&self) -> StateResult<Option<Vec<String>>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT source_task_ids FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        match result.flatten() {
            Some(json) => {
                let ids: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
                Ok(Some(ids))
            }
            None => Ok(None),
        }
    }

    /// Set the source task IDs
    pub async fn set_source_task_ids(&self, task_ids: Option<&[String]>) -> StateResult<()> {
        let pool = self.pool().await;
        let json = task_ids.map(|ids| serde_json::to_string(ids).unwrap_or_default());
        sqlx::query("UPDATE state SET source_task_ids = ?, updated_at = ? WHERE id = 1")
            .bind(json)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the board snapshot (JSON snapshot of board state at dispatch)
    pub async fn get_board_snapshot(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT board_snapshot FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the board snapshot
    pub async fn set_board_snapshot(&self, snapshot: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET board_snapshot = ?, updated_at = ? WHERE id = 1")
            .bind(snapshot)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the branch-off commit SHA
    pub async fn get_branch_off_commit(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT branch_off_commit FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the branch-off commit SHA
    pub async fn set_branch_off_commit(&self, commit: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET branch_off_commit = ?, updated_at = ? WHERE id = 1")
            .bind(commit)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Delivery Tracking
    // =========================================================================

    /// Get the delivery status
    pub async fn get_delivery_status(&self) -> StateResult<super::types::DeliveryStatus> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT delivery_status FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        match result.flatten() {
            Some(val) => Ok(super::types::DeliveryStatus::from_str(&val)
                .unwrap_or(super::types::DeliveryStatus::Pending)),
            None => Ok(super::types::DeliveryStatus::Pending),
        }
    }

    /// Set the delivery status
    pub async fn set_delivery_status(
        &self,
        status: super::types::DeliveryStatus,
    ) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET delivery_status = ?, updated_at = ? WHERE id = 1")
            .bind(status.as_str())
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the delivery branch name
    pub async fn get_delivery_branch(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT delivery_branch FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the delivery branch name
    pub async fn set_delivery_branch(&self, branch: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET delivery_branch = ?, updated_at = ? WHERE id = 1")
            .bind(branch)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the PR URL
    pub async fn get_pr_url(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT pr_url FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the PR URL
    pub async fn set_pr_url(&self, url: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET pr_url = ?, updated_at = ? WHERE id = 1")
            .bind(url)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the PR number
    pub async fn get_pr_number(&self) -> StateResult<Option<i64>> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT pr_number FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the PR number
    pub async fn set_pr_number(&self, number: Option<i64>) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET pr_number = ?, updated_at = ? WHERE id = 1")
            .bind(number)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the merged_at timestamp
    pub async fn get_merged_at(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT merged_at FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the merged_at timestamp
    pub async fn set_merged_at(&self, timestamp: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        let ts = timestamp.map(|s| s.to_string()).or_else(|| Some(utc_now()));
        sqlx::query("UPDATE state SET merged_at = ?, updated_at = ? WHERE id = 1")
            .bind(ts)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the abandoned_at timestamp
    pub async fn get_abandoned_at(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT abandoned_at FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten())
    }

    /// Set the abandoned_at timestamp
    pub async fn set_abandoned_at(&self, timestamp: Option<&str>) -> StateResult<()> {
        let pool = self.pool().await;
        let ts = timestamp.map(|s| s.to_string()).or_else(|| Some(utc_now()));
        sqlx::query("UPDATE state SET abandoned_at = ?, updated_at = ? WHERE id = 1")
            .bind(ts)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Merge State
    // =========================================================================

    /// Get the staleness commits count
    pub async fn get_staleness_commits(&self) -> StateResult<u32> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT staleness_commits FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten().unwrap_or(0) as u32)
    }

    /// Set the staleness commits count
    pub async fn set_staleness_commits(&self, count: u32) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET staleness_commits = ?, updated_at = ? WHERE id = 1")
            .bind(count as i64)
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get the merge state
    pub async fn get_merge_state(&self) -> StateResult<super::types::MergeState> {
        let pool = self.pool().await;
        let result: Option<Option<String>> =
            sqlx::query_scalar("SELECT merge_state FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        match result.flatten() {
            Some(val) => Ok(super::types::MergeState::from_str(&val)
                .unwrap_or(super::types::MergeState::Unknown)),
            None => Ok(super::types::MergeState::Unknown),
        }
    }

    /// Set the merge state
    pub async fn set_merge_state(&self, state: super::types::MergeState) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET merge_state = ?, updated_at = ? WHERE id = 1")
            .bind(state.as_str())
            .bind(utc_now())
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Mark run as delivered (set delivery_status and merged_at)
    pub async fn mark_delivered(&self) -> StateResult<()> {
        self.set_delivery_status(super::types::DeliveryStatus::Merged)
            .await?;
        self.set_merged_at(None).await?;
        self.set_status(Status::Delivered).await?;
        self.log_history("delivered", None).await?;
        Ok(())
    }

    /// Mark run as abandoned
    pub async fn mark_abandoned(&self) -> StateResult<()> {
        self.set_delivery_status(super::types::DeliveryStatus::Abandoned)
            .await?;
        self.set_abandoned_at(None).await?;
        self.log_history("abandoned", None).await?;
        Ok(())
    }

    // =========================================================================
    // Scaling Check (Event-Driven Worker Spawning)
    // =========================================================================

    /// Request a scaling check on the next daemon poll.
    pub async fn request_scaling_check(&self) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "UPDATE state SET scaling_check_requested = scaling_check_requested + 1 WHERE id = 1",
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    /// Consume the scaling check counter, returning whether any checks were requested.
    pub async fn consume_scaling_check(&self) -> StateResult<bool> {
        let pool = self.pool().await;
        let count: i64 =
            sqlx::query_scalar("SELECT scaling_check_requested FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?
                .flatten()
                .unwrap_or(0);

        if count > 0 {
            sqlx::query("UPDATE state SET scaling_check_requested = 0 WHERE id = 1")
                .execute(&pool)
                .await?;
        }
        Ok(count > 0)
    }

    // =========================================================================
    // Run Summary (for efficient status queries)
    // =========================================================================

    /// Get all run summary data in an optimized single fetch.
    /// Note: unread_count is always 0 as messaging uses project-level storage.
    pub async fn get_run_summary(&self) -> StateResult<super::types::RunStateSummary> {
        use chrono::{DateTime, Utc};
        use sqlx::Row;

        let pool = self.pool().await;

        // Query 1: Get all needed state columns in one query
        let state_row = sqlx::query(
            "SELECT status, created_at, updated_at, started_at, time_limit_minutes, worker_scale FROM state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await?;

        let status: String = state_row.get("status");
        let created_at: Option<String> = state_row.get("created_at");
        let updated_at: Option<String> = state_row.get("updated_at");
        let started_at: Option<String> = state_row.get("started_at");
        let time_limit_minutes: Option<i64> = state_row.get("time_limit_minutes");
        let worker_scale: Option<String> = state_row.get("worker_scale");

        // Query 2: Get worker counts
        let worker_row = sqlx::query(
            "SELECT COUNT(*) as total, SUM(CASE WHEN status = 'working' THEN 1 ELSE 0 END) as active FROM workers",
        )
        .fetch_one(&pool)
        .await?;

        let workers_registered: i64 = worker_row.get("total");
        let workers_active: Option<i64> = worker_row.get("active");

        // workers_total is the actual number of worker records for the run.
        let workers_total = workers_registered as u32;

        // workers_desired is derived from worker_scale, falling back to workers_total.
        let workers_desired = worker_scale
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(workers_total);

        // Parse status
        let status = super::types::Status::from_str(&status).unwrap_or(super::types::Status::Draft);

        // Calculate elapsed minutes based on status
        let elapsed_minutes = if status == super::types::Status::Draft {
            0.0
        } else if let Some(ref sa) = started_at {
            if let Ok(start_time) = DateTime::parse_from_rfc3339(sa) {
                let elapsed = Utc::now().signed_duration_since(start_time);
                elapsed.num_seconds() as f64 / 60.0
            } else {
                0.0
            }
        } else if let Some(ref ca) = created_at {
            if let Ok(start_time) = DateTime::parse_from_rfc3339(ca) {
                let elapsed = Utc::now().signed_duration_since(start_time);
                elapsed.num_seconds() as f64 / 60.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(super::types::RunStateSummary {
            status,
            created_at,
            updated_at,
            started_at,
            time_limit_minutes,
            unread_count: 0, // Always 0 - messaging uses project-level storage
            workers_active: workers_active.unwrap_or(0) as u32,
            workers_total,
            workers_desired,
            elapsed_minutes,
        })
    }
}
