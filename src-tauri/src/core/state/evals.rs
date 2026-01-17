//! Eval methods
//!
//! Methods for managing evaluations: starting, completing, querying.

use rusqlite::{params, Row};

use super::types::{Eval, EvalStatus, StateResult};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // Eval Methods
    // =========================================================================

    pub(super) fn eval_from_row(row: &Row) -> rusqlite::Result<Eval> {
        Ok(Eval {
            id: row.get("id")?,
            branch: row.get("branch")?,
            eval_name: row.get("eval_name")?,
            status: EvalStatus::from_str(&row.get::<_, String>("status")?)
                .unwrap_or(EvalStatus::Running),
            feedback: row.get("feedback")?,
            log_file: row.get("log_file")?,
            started_at: row.get("started_at")?,
            finished_at: row.get("finished_at")?,
            pid: row.get::<_, Option<i64>>("pid")?.map(|p| p as u32),
        })
    }

    /// Start a new eval
    pub fn start_eval(
        &self,
        branch: &str,
        eval_name: Option<&str>,
        log_file: Option<&str>,
    ) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO evals (branch, status, started_at, eval_name, log_file) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![branch, EvalStatus::Running.as_str(), self.now(), eval_name, log_file],
        )?;
        let id = self.db.last_insert_rowid();
        self.log_history("eval_start", eval_name.map(|n| n.to_string()).as_deref())?;
        Ok(id)
    }

    /// Complete an eval
    pub fn complete_eval(&self, eval_id: i64, success: bool, feedback: &str) -> StateResult<()> {
        let status = if success {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        };
        self.db.execute(
            "UPDATE evals SET status = ?1, feedback = ?2, finished_at = ?3 WHERE id = ?4",
            params![status.as_str(), feedback, self.now(), eval_id],
        )?;
        self.log_history("eval_complete", Some(&format!("{}", status)))?;
        Ok(())
    }

    /// Get an eval by ID
    pub fn get_eval(&self, eval_id: i64) -> StateResult<Option<Eval>> {
        let result = self.db.query_row(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals WHERE id = ?1",
            params![eval_id],
            Self::eval_from_row,
        );
        match result {
            Ok(eval) => Ok(Some(eval)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get eval by name
    pub fn get_eval_by_name(&self, eval_name: &str) -> StateResult<Option<Eval>> {
        let result = self.db.query_row(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals WHERE eval_name = ?1 ORDER BY id DESC LIMIT 1",
            params![eval_name],
            Self::eval_from_row,
        );
        match result {
            Ok(eval) => Ok(Some(eval)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get all evals
    pub fn get_evals(&self, limit: i64) -> StateResult<Vec<Eval>> {
        let mut stmt = self.db.prepare(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals ORDER BY id ASC LIMIT ?1"
        )?;
        let evals = stmt
            .query_map(params![limit], Self::eval_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(evals)
    }

    /// Get running eval
    pub fn get_running_eval(&self) -> StateResult<Option<Eval>> {
        let result = self.db.query_row(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals WHERE status = ?1 ORDER BY id DESC LIMIT 1",
            params![EvalStatus::Running.as_str()],
            Self::eval_from_row,
        );
        match result {
            Ok(eval) => Ok(Some(eval)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set the PID of a running eval
    pub fn set_eval_pid(&self, eval_id: i64, pid: u32) -> StateResult<()> {
        self.db.execute(
            "UPDATE evals SET pid = ?1 WHERE id = ?2",
            params![pid as i64, eval_id],
        )?;
        Ok(())
    }

    /// Cancel running evals and kill their processes
    pub fn cancel_running_evals(&self, reason: &str) -> StateResult<i64> {
        let mut stmt = self
            .db
            .prepare("SELECT id, eval_name, pid FROM evals WHERE status = ?1")?;
        let running: Vec<(i64, Option<String>, Option<i64>)> = stmt
            .query_map(params![EvalStatus::Running.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        for (id, name, pid) in &running {
            // Kill the eval process if we have a PID
            if let Some(pid) = pid {
                let pid = *pid as u32;
                #[cfg(unix)]
                {
                    use tracing::info;
                    // Kill the process group (eval runs in its own process group)
                    info!("Killing eval process group {}", pid);
                    unsafe {
                        // SIGTERM to process group
                        libc::kill(-(pid as i32), libc::SIGTERM);
                    }
                    // Give it a moment then SIGKILL
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
            }

            self.db.execute(
                "UPDATE evals SET status = ?1, feedback = ?2, finished_at = ?3 WHERE id = ?4",
                params![EvalStatus::Failed.as_str(), reason, self.now(), id],
            )?;
            self.log_history("eval_cancel", name.as_deref())?;
        }

        Ok(running.len() as i64)
    }
}
