//! Worker methods
//!
//! Methods for managing workers: adding, updating, querying status.

use sqlx::Row;

use super::types::{StateError, StateResult, Status, Worker, WorkerStatus, WorkerUpdate};
use super::SQLiteState;
use crate::core::db::utc_now;

impl SQLiteState {
    // =========================================================================
    // Worker Methods
    // =========================================================================

    /// Add a new worker
    pub async fn add_worker(
        &self,
        name: &str,
        work_dir: &str,
        location: &str,
        capability_profile: Option<crate::core::CapabilityProfile>,
    ) -> StateResult<Option<Worker>> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "INSERT INTO workers (name, status, work_dir, location, created_at, capability_profile) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(WorkerStatus::Working.as_str())
        .bind(work_dir)
        .bind(location)
        .bind(utc_now())
        .bind(capability_profile.map(|profile| profile.as_str()))
        .execute(&pool)
        .await;

        match result {
            Ok(_) => {
                self.log_history("worker_add", Some(name)).await?;
                self.get_worker(name).await
            }
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                Ok(None) // Already exists
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Get a worker by name
    pub async fn get_worker(&self, name: &str) -> StateResult<Option<Worker>> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id, capability_profile FROM workers WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&pool)
        .await?;

        Ok(result.map(|row| Worker {
            id: row.get("id"),
            name: row.get("name"),
            pid: row.get("pid"),
            runner_id: row.get("runner_id"),
            runner_type: row.get("runner_type"),
            session_id: row.get("session_id"),
            session_started_at: row.get("session_started_at"),
            status: WorkerStatus::from_str(&row.get::<String, _>("status"))
                .unwrap_or(WorkerStatus::Awaiting),
            work_dir: row.get("work_dir"),
            waiting_thread: row.get("waiting_thread"),
            needs_restart: row
                .get::<Option<i64>, _>("needs_restart")
                .map(|v| v != 0)
                .unwrap_or(false),
            location: row
                .get::<Option<String>, _>("location")
                .unwrap_or_else(|| "local".to_string()),
            last_heartbeat: row.get("last_heartbeat"),
            created_at: row.get("created_at"),
            hitl_waiting: row
                .get::<Option<i64>, _>("hitl_waiting")
                .map(|v| v != 0)
                .unwrap_or(false),
            state_handle: row.get("state_handle"),
            assigned_task_id: row.get("assigned_task_id"),
            last_task_id: row.get("last_task_id"),
            capability_profile: row
                .get::<Option<String>, _>("capability_profile")
                .and_then(|value| crate::core::CapabilityProfile::from_str(&value)),
        }))
    }

    /// Get all workers
    pub async fn get_workers(&self) -> StateResult<Vec<Worker>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id, capability_profile FROM workers ORDER BY id",
        )
        .fetch_all(&pool)
        .await?;

        let workers = rows
            .into_iter()
            .map(|row| Worker {
                id: row.get("id"),
                name: row.get("name"),
                pid: row.get("pid"),
                runner_id: row.get("runner_id"),
                runner_type: row.get("runner_type"),
                session_id: row.get("session_id"),
                session_started_at: row.get("session_started_at"),
                status: WorkerStatus::from_str(&row.get::<String, _>("status"))
                    .unwrap_or(WorkerStatus::Awaiting),
                work_dir: row.get("work_dir"),
                waiting_thread: row.get("waiting_thread"),
                needs_restart: row
                    .get::<Option<i64>, _>("needs_restart")
                    .map(|v| v != 0)
                    .unwrap_or(false),
                location: row
                    .get::<Option<String>, _>("location")
                    .unwrap_or_else(|| "local".to_string()),
                last_heartbeat: row.get("last_heartbeat"),
                created_at: row.get("created_at"),
                hitl_waiting: row
                    .get::<Option<i64>, _>("hitl_waiting")
                    .map(|v| v != 0)
                    .unwrap_or(false),
                state_handle: row.get("state_handle"),
                assigned_task_id: row.get("assigned_task_id"),
                last_task_id: row.get("last_task_id"),
                capability_profile: row
                    .get::<Option<String>, _>("capability_profile")
                    .and_then(|value| crate::core::CapabilityProfile::from_str(&value)),
            })
            .collect();

        Ok(workers)
    }

    /// Get active workers (not awaiting or error)
    pub async fn get_active_workers(&self) -> StateResult<Vec<Worker>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id, capability_profile FROM workers WHERE status NOT IN (?, ?) ORDER BY id",
        )
        .bind(WorkerStatus::Awaiting.as_str())
        .bind(WorkerStatus::Error.as_str())
        .fetch_all(&pool)
        .await?;

        let workers = rows
            .into_iter()
            .map(|row| Worker {
                id: row.get("id"),
                name: row.get("name"),
                pid: row.get("pid"),
                runner_id: row.get("runner_id"),
                runner_type: row.get("runner_type"),
                session_id: row.get("session_id"),
                session_started_at: row.get("session_started_at"),
                status: WorkerStatus::from_str(&row.get::<String, _>("status"))
                    .unwrap_or(WorkerStatus::Awaiting),
                work_dir: row.get("work_dir"),
                waiting_thread: row.get("waiting_thread"),
                needs_restart: row
                    .get::<Option<i64>, _>("needs_restart")
                    .map(|v| v != 0)
                    .unwrap_or(false),
                location: row
                    .get::<Option<String>, _>("location")
                    .unwrap_or_else(|| "local".to_string()),
                last_heartbeat: row.get("last_heartbeat"),
                created_at: row.get("created_at"),
                hitl_waiting: row
                    .get::<Option<i64>, _>("hitl_waiting")
                    .map(|v| v != 0)
                    .unwrap_or(false),
                state_handle: row.get("state_handle"),
                assigned_task_id: row.get("assigned_task_id"),
                last_task_id: row.get("last_task_id"),
                capability_profile: row
                    .get::<Option<String>, _>("capability_profile")
                    .and_then(|value| crate::core::CapabilityProfile::from_str(&value)),
            })
            .collect();

        Ok(workers)
    }

    /// Update worker fields
    pub async fn update_worker(&self, name: &str, updates: WorkerUpdate) -> StateResult<()> {
        let pool = self.pool().await;

        // Build dynamic query
        let mut set_clauses = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(pid) = updates.pid {
            match pid {
                Some(v) => {
                    set_clauses.push("pid = ?");
                    params.push(v.to_string());
                }
                None => {
                    set_clauses.push("pid = NULL");
                }
            }
        }
        if let Some(ref runner_id) = updates.runner_id {
            set_clauses.push("runner_id = ?");
            params.push(runner_id.clone());
        }
        if let Some(ref runner_type) = updates.runner_type {
            set_clauses.push("runner_type = ?");
            params.push(runner_type.clone());
        }
        if let Some(ref session_id) = updates.session_id {
            set_clauses.push("session_id = ?");
            params.push(session_id.clone());
            set_clauses.push("session_started_at = ?");
            params.push(utc_now());
        }
        if let Some(status) = updates.status {
            // Check old status for history logging
            let old_status = self.get_worker(name).await?.map(|w| w.status);
            set_clauses.push("status = ?");
            params.push(status.as_str().to_string());

            // Log status change if different
            if old_status != Some(status) {
                self.log_history("worker_status", Some(&format!("{} → {}", name, status)))
                    .await?;
            }
        }
        if let Some(ref waiting_thread) = updates.waiting_thread {
            set_clauses.push("waiting_thread = ?");
            params.push(waiting_thread.clone());
        }
        if let Some(needs_restart) = updates.needs_restart {
            set_clauses.push("needs_restart = ?");
            params.push(if needs_restart { "1" } else { "0" }.to_string());
        }
        if let Some(ref last_heartbeat) = updates.last_heartbeat {
            set_clauses.push("last_heartbeat = ?");
            params.push(last_heartbeat.clone());
        }
        if let Some(hitl_waiting) = updates.hitl_waiting {
            set_clauses.push("hitl_waiting = ?");
            params.push(if hitl_waiting { "1" } else { "0" }.to_string());
        }
        if let Some(ref state_handle) = updates.state_handle {
            set_clauses.push("state_handle = ?");
            params.push(state_handle.clone().unwrap_or_default());
        }
        if let Some(ref assigned_task_id) = updates.assigned_task_id {
            set_clauses.push("assigned_task_id = ?");
            params.push(assigned_task_id.clone().unwrap_or_default());
        }
        if let Some(ref last_task_id) = updates.last_task_id {
            set_clauses.push("last_task_id = ?");
            params.push(last_task_id.clone().unwrap_or_default());
        }
        if let Some(ref capability_profile) = updates.capability_profile {
            match capability_profile {
                Some(profile) => {
                    set_clauses.push("capability_profile = ?");
                    params.push(profile.as_str().to_string());
                }
                None => set_clauses.push("capability_profile = NULL"),
            }
        }

        if set_clauses.is_empty() {
            return Ok(());
        }

        // Build the query manually since we need dynamic columns
        // This is safe as column names are hardcoded
        let sql = format!(
            "UPDATE workers SET {} WHERE name = ?",
            set_clauses.join(", ")
        );

        // Execute with dynamic binding
        let mut query = sqlx::query(&sql);
        for param in &params {
            query = query.bind(param);
        }
        query = query.bind(name);
        query.execute(&pool).await?;

        Ok(())
    }

    /// Check if all workers are inactive (awaiting or error)
    pub async fn all_workers_inactive(&self) -> StateResult<bool> {
        let pool = self.pool().await;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM workers WHERE status NOT IN (?, ?)")
                .bind(WorkerStatus::Awaiting.as_str())
                .bind(WorkerStatus::Error.as_str())
                .fetch_one(&pool)
                .await?;
        Ok(count == 0)
    }

    /// Pause all workers
    pub async fn pause_all_workers(&self, reason: &str) -> StateResult<()> {
        self.set_status(Status::Paused).await?;
        self.set_waiting_reason(Some(reason)).await?;

        let workers = self.get_workers().await?;
        for w in workers {
            if w.status == WorkerStatus::Working {
                self.update_worker(
                    &w.name,
                    WorkerUpdate {
                        status: Some(WorkerStatus::Paused),
                        ..Default::default()
                    },
                )
                .await?;
            }
        }

        let detail = if reason.len() > 100 {
            &reason[..100]
        } else {
            reason
        };
        self.log_history("pause_all", Some(detail)).await?;
        Ok(())
    }

    /// Resume all workers
    pub async fn resume_all_workers(&self) -> StateResult<()> {
        self.set_status(Status::Working).await?;
        self.set_waiting_reason(None).await?;

        let workers = self.get_workers().await?;
        for w in workers {
            if w.status == WorkerStatus::Paused {
                self.update_worker(
                    &w.name,
                    WorkerUpdate {
                        status: Some(WorkerStatus::Working),
                        ..Default::default()
                    },
                )
                .await?;
            }
        }

        self.log_history("resume_all", None).await?;
        Ok(())
    }

    /// Get count of workers waiting for HITL input
    pub async fn get_waiting_count(&self) -> StateResult<i64> {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workers WHERE hitl_waiting = 1")
            .fetch_one(&pool)
            .await?;
        Ok(count)
    }

    /// Get workers waiting for HITL input
    pub async fn get_hitl_waiting_workers(&self) -> StateResult<Vec<Worker>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id, capability_profile FROM workers WHERE hitl_waiting = 1 ORDER BY id",
        )
        .fetch_all(&pool)
        .await?;

        let workers = rows
            .into_iter()
            .map(|row| Worker {
                id: row.get("id"),
                name: row.get("name"),
                pid: row.get("pid"),
                runner_id: row.get("runner_id"),
                runner_type: row.get("runner_type"),
                session_id: row.get("session_id"),
                session_started_at: row.get("session_started_at"),
                status: WorkerStatus::from_str(&row.get::<String, _>("status"))
                    .unwrap_or(WorkerStatus::Awaiting),
                work_dir: row.get("work_dir"),
                waiting_thread: row.get("waiting_thread"),
                needs_restart: row
                    .get::<Option<i64>, _>("needs_restart")
                    .map(|v| v != 0)
                    .unwrap_or(false),
                location: row
                    .get::<Option<String>, _>("location")
                    .unwrap_or_else(|| "local".to_string()),
                last_heartbeat: row.get("last_heartbeat"),
                created_at: row.get("created_at"),
                hitl_waiting: row
                    .get::<Option<i64>, _>("hitl_waiting")
                    .map(|v| v != 0)
                    .unwrap_or(false),
                state_handle: row.get("state_handle"),
                assigned_task_id: row.get("assigned_task_id"),
                last_task_id: row.get("last_task_id"),
                capability_profile: row
                    .get::<Option<String>, _>("capability_profile")
                    .and_then(|value| crate::core::CapabilityProfile::from_str(&value)),
            })
            .collect();

        Ok(workers)
    }

    /// Clear HITL waiting state for all workers
    pub async fn clear_all_hitl_waiting(&self) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "UPDATE workers SET hitl_waiting = 0, waiting_thread = NULL WHERE hitl_waiting = 1",
        )
        .execute(&pool)
        .await?;
        Ok(())
    }

    /// Delete a worker from the database.
    pub async fn delete_worker(&self, name: &str) -> StateResult<()> {
        let pool = self.pool().await;
        let result = sqlx::query("DELETE FROM workers WHERE name = ?")
            .bind(name)
            .execute(&pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(StateError::NotFound(format!("Worker '{}' not found", name)));
        }

        self.log_history("worker_delete", Some(name)).await?;
        Ok(())
    }
}
