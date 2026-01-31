//! Worker methods
//!
//! Methods for managing workers: adding, updating, querying status.

use rusqlite::{params, Row};

use super::types::{StateResult, Status, Worker, WorkerStatus, WorkerUpdate};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // Worker Methods
    // =========================================================================

    pub(super) fn worker_from_row(row: &Row) -> rusqlite::Result<Worker> {
        Ok(Worker {
            id: row.get("id")?,
            name: row.get("name")?,
            pid: row.get("pid")?,
            runner_id: row.get("runner_id")?,
            runner_type: row.get("runner_type")?,
            session_id: row.get("session_id")?,
            session_started_at: row.get("session_started_at")?,
            status: WorkerStatus::from_str(&row.get::<_, String>("status")?)
                .unwrap_or(WorkerStatus::Awaiting),
            work_dir: row.get("work_dir")?,
            waiting_thread: row.get("waiting_thread")?,
            needs_restart: row
                .get::<_, Option<i64>>("needs_restart")?
                .map(|v| v != 0)
                .unwrap_or(false),
            location: row
                .get::<_, Option<String>>("location")?
                .unwrap_or_else(|| "local".to_string()),
            last_heartbeat: row.get("last_heartbeat")?,
            created_at: row.get("created_at")?,
            hitl_waiting: row
                .get::<_, Option<i64>>("hitl_waiting")?
                .map(|v| v != 0)
                .unwrap_or(false),
            state_handle: row.get("state_handle")?,
            assigned_task_id: row.get("assigned_task_id")?,
            last_task_id: row.get("last_task_id")?,
        })
    }

    /// Add a new worker
    pub fn add_worker(
        &self,
        name: &str,
        work_dir: &str,
        location: &str,
    ) -> StateResult<Option<Worker>> {
        match self.db.execute(
            "INSERT INTO workers (name, status, work_dir, location, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, WorkerStatus::Working.as_str(), work_dir, location, self.now()],
        ) {
            Ok(_) => {
                self.log_history("worker_add", Some(name))?;
                self.get_worker(name)
            }
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.extended_code == 1555 => {
                Ok(None) // Already exists
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Get a worker by name
    pub fn get_worker(&self, name: &str) -> StateResult<Option<Worker>> {
        let result = self.db.query_row(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id FROM workers WHERE name = ?1",
            params![name],
            Self::worker_from_row,
        );
        match result {
            Ok(worker) => Ok(Some(worker)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get all workers
    pub fn get_workers(&self) -> StateResult<Vec<Worker>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id FROM workers ORDER BY id"
        )?;
        let workers = stmt
            .query_map([], Self::worker_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(workers)
    }

    /// Get active workers (not awaiting or error)
    pub fn get_active_workers(&self) -> StateResult<Vec<Worker>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id FROM workers WHERE status NOT IN (?1, ?2) ORDER BY id"
        )?;
        let workers = stmt
            .query_map(
                params![
                    WorkerStatus::Awaiting.as_str(),
                    WorkerStatus::Error.as_str()
                ],
                Self::worker_from_row,
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
        if let Some(runner_id) = &updates.runner_id {
            set_clauses.push("runner_id = ?");
            params_vec.push(Box::new(runner_id.clone()));
        }
        if let Some(runner_type) = &updates.runner_type {
            set_clauses.push("runner_type = ?");
            params_vec.push(Box::new(runner_type.clone()));
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
        if let Some(hitl_waiting) = updates.hitl_waiting {
            set_clauses.push("hitl_waiting = ?");
            params_vec.push(Box::new(if hitl_waiting { 1i64 } else { 0i64 }));
        }
        if let Some(ref state_handle) = updates.state_handle {
            set_clauses.push("state_handle = ?");
            params_vec.push(Box::new(state_handle.clone()));
        }
        if let Some(ref assigned_task_id) = updates.assigned_task_id {
            set_clauses.push("assigned_task_id = ?");
            params_vec.push(Box::new(assigned_task_id.clone()));
        }
        if let Some(ref last_task_id) = updates.last_task_id {
            set_clauses.push("last_task_id = ?");
            params_vec.push(Box::new(last_task_id.clone()));
        }

        if set_clauses.is_empty() {
            return Ok(());
        }

        params_vec.push(Box::new(name.to_string()));
        let sql = format!(
            "UPDATE workers SET {} WHERE name = ?",
            set_clauses.join(", ")
        );

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        self.db.execute(&sql, params_refs.as_slice())?;
        Ok(())
    }

    /// Check if all workers are inactive (awaiting or error)
    pub fn all_workers_inactive(&self) -> StateResult<bool> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM workers WHERE status NOT IN (?1, ?2)",
            params![
                WorkerStatus::Awaiting.as_str(),
                WorkerStatus::Error.as_str()
            ],
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
                self.update_worker(
                    &w.name,
                    WorkerUpdate {
                        status: Some(WorkerStatus::Paused),
                        ..Default::default()
                    },
                )?;
            }
        }

        let detail = if reason.len() > 100 {
            &reason[..100]
        } else {
            reason
        };
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
                self.update_worker(
                    &w.name,
                    WorkerUpdate {
                        status: Some(WorkerStatus::Working),
                        ..Default::default()
                    },
                )?;
            }
        }

        self.log_history("resume_all", None)?;
        Ok(())
    }

    /// Get count of workers waiting for HITL input
    pub fn get_waiting_count(&self) -> StateResult<i64> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM workers WHERE hitl_waiting = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get workers waiting for HITL input
    pub fn get_hitl_waiting_workers(&self) -> StateResult<Vec<Worker>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, pid, runner_id, runner_type, session_id, session_started_at, status, work_dir, waiting_thread, needs_restart, location, last_heartbeat, created_at, hitl_waiting, state_handle, assigned_task_id, last_task_id FROM workers WHERE hitl_waiting = 1 ORDER BY id"
        )?;
        let workers = stmt
            .query_map([], Self::worker_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(workers)
    }

    /// Clear HITL waiting state for all workers
    ///
    /// Called when the user resumes a run to allow all workers waiting
    /// for human input to continue.
    pub fn clear_all_hitl_waiting(&self) -> StateResult<()> {
        self.db.execute(
            "UPDATE workers SET hitl_waiting = 0, waiting_thread = NULL WHERE hitl_waiting = 1",
            [],
        )?;
        Ok(())
    }
}
