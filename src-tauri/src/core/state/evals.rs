//! Eval methods
//!
//! Methods for managing evaluations: starting, completing, querying.

use sqlx::Row;

use super::types::{Eval, EvalStatus, StateResult};
use super::SQLiteState;
use crate::core::db::utc_now;

impl SQLiteState {
    // =========================================================================
    // Eval Methods
    // =========================================================================

    /// Start a new eval
    pub async fn start_eval(
        &self,
        branch: &str,
        eval_name: Option<&str>,
        log_file: Option<&str>,
    ) -> StateResult<i64> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "INSERT INTO evals (branch, status, started_at, eval_name, log_file) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(branch)
        .bind(EvalStatus::Running.as_str())
        .bind(utc_now())
        .bind(eval_name)
        .bind(log_file)
        .execute(&pool)
        .await?;
        let id = result.last_insert_rowid();
        self.log_history("eval_start", eval_name).await?;
        Ok(id)
    }

    /// Complete an eval
    pub async fn complete_eval(
        &self,
        eval_id: i64,
        success: bool,
        feedback: &str,
    ) -> StateResult<()> {
        let pool = self.pool().await;
        let status = if success {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        };
        sqlx::query("UPDATE evals SET status = ?, feedback = ?, finished_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(feedback)
            .bind(utc_now())
            .bind(eval_id)
            .execute(&pool)
            .await?;
        self.log_history("eval_complete", Some(&format!("{}", status)))
            .await?;
        Ok(())
    }

    /// Get an eval by ID
    pub async fn get_eval(&self, eval_id: i64) -> StateResult<Option<Eval>> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals WHERE id = ?",
        )
        .bind(eval_id)
        .fetch_optional(&pool)
        .await?;

        Ok(result.map(|row| Eval {
            id: row.get("id"),
            branch: row.get("branch"),
            eval_name: row.get("eval_name"),
            status: EvalStatus::from_str(&row.get::<String, _>("status"))
                .unwrap_or(EvalStatus::Running),
            feedback: row.get("feedback"),
            log_file: row.get("log_file"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
            pid: row.get::<Option<i64>, _>("pid").map(|p| p as u32),
        }))
    }

    /// Get eval by name
    pub async fn get_eval_by_name(&self, eval_name: &str) -> StateResult<Option<Eval>> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals WHERE eval_name = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(eval_name)
        .fetch_optional(&pool)
        .await?;

        Ok(result.map(|row| Eval {
            id: row.get("id"),
            branch: row.get("branch"),
            eval_name: row.get("eval_name"),
            status: EvalStatus::from_str(&row.get::<String, _>("status"))
                .unwrap_or(EvalStatus::Running),
            feedback: row.get("feedback"),
            log_file: row.get("log_file"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
            pid: row.get::<Option<i64>, _>("pid").map(|p| p as u32),
        }))
    }

    /// Get all evals
    pub async fn get_evals(&self, limit: i64) -> StateResult<Vec<Eval>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals ORDER BY id ASC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&pool)
        .await?;

        let evals = rows
            .into_iter()
            .map(|row| Eval {
                id: row.get("id"),
                branch: row.get("branch"),
                eval_name: row.get("eval_name"),
                status: EvalStatus::from_str(&row.get::<String, _>("status"))
                    .unwrap_or(EvalStatus::Running),
                feedback: row.get("feedback"),
                log_file: row.get("log_file"),
                started_at: row.get("started_at"),
                finished_at: row.get("finished_at"),
                pid: row.get::<Option<i64>, _>("pid").map(|p| p as u32),
            })
            .collect();
        Ok(evals)
    }

    /// Get running eval
    pub async fn get_running_eval(&self) -> StateResult<Option<Eval>> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "SELECT id, branch, status, feedback, started_at, finished_at, eval_name, log_file, pid FROM evals WHERE status = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(EvalStatus::Running.as_str())
        .fetch_optional(&pool)
        .await?;

        Ok(result.map(|row| Eval {
            id: row.get("id"),
            branch: row.get("branch"),
            eval_name: row.get("eval_name"),
            status: EvalStatus::from_str(&row.get::<String, _>("status"))
                .unwrap_or(EvalStatus::Running),
            feedback: row.get("feedback"),
            log_file: row.get("log_file"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
            pid: row.get::<Option<i64>, _>("pid").map(|p| p as u32),
        }))
    }

    /// Set the PID of a running eval
    pub async fn set_eval_pid(&self, eval_id: i64, pid: u32) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE evals SET pid = ? WHERE id = ?")
            .bind(pid as i64)
            .bind(eval_id)
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Check if there's a cancelled eval that was paused (feedback = "Run paused")
    pub async fn has_paused_eval(&self) -> StateResult<bool> {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM evals WHERE status = ? AND feedback = 'Run paused'",
        )
        .bind(EvalStatus::Failed.as_str())
        .fetch_one(&pool)
        .await?;
        Ok(count > 0)
    }

    /// Clear the "Run paused" feedback from cancelled evals (called after resuming)
    pub async fn clear_paused_evals(&self) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "UPDATE evals SET feedback = 'Paused and resumed' WHERE status = ? AND feedback = 'Run paused'",
        )
        .bind(EvalStatus::Failed.as_str())
        .execute(&pool)
        .await?;
        Ok(())
    }

    /// Cancel running evals and kill their processes
    pub async fn cancel_running_evals(&self, reason: &str) -> StateResult<i64> {
        let pool = self.pool().await;
        let rows = sqlx::query("SELECT id, eval_name, pid FROM evals WHERE status = ?")
            .bind(EvalStatus::Running.as_str())
            .fetch_all(&pool)
            .await?;

        let running: Vec<(i64, Option<String>, Option<i64>)> = rows
            .into_iter()
            .map(|row| (row.get("id"), row.get("eval_name"), row.get("pid")))
            .collect();

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

            sqlx::query("UPDATE evals SET status = ?, feedback = ?, finished_at = ? WHERE id = ?")
                .bind(EvalStatus::Failed.as_str())
                .bind(reason)
                .bind(utc_now())
                .bind(id)
                .execute(&pool)
                .await?;
            self.log_history("eval_cancel", name.as_deref()).await?;
        }

        Ok(running.len() as i64)
    }
}
