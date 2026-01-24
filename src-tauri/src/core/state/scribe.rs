//! Scribe submission methods
//!
//! Methods for managing scribe submissions - learnings recorded by workers
//! that are batched and processed by an ephemeral Scribe agent.

use rusqlite::{params, Row};

use super::types::StateResult;
use super::SQLiteState;

/// A scribe submission from a worker
#[derive(Debug, Clone)]
pub struct ScribeSubmission {
    pub id: i64,
    pub worker_name: String,
    pub content: String,
    pub status: String,
    pub batch_id: Option<i64>,
    pub retry_count: i32,
    pub created_at: String,
    pub processed_at: Option<String>,
}

impl SQLiteState {
    // =========================================================================
    // Scribe Submission Methods
    // =========================================================================

    fn submission_from_row(row: &Row) -> rusqlite::Result<ScribeSubmission> {
        Ok(ScribeSubmission {
            id: row.get("id")?,
            worker_name: row.get("worker_name")?,
            content: row.get("content")?,
            status: row.get("status")?,
            batch_id: row.get("batch_id")?,
            retry_count: row.get("retry_count")?,
            created_at: row.get("created_at")?,
            processed_at: row.get("processed_at")?,
        })
    }

    /// Add a scribe submission from a worker
    ///
    /// If this is the first pending submission, also sets scribe_batch_started_at
    pub fn add_scribe_submission(&self, worker_name: &str, content: &str) -> StateResult<i64> {
        let now = self.now();

        // Insert the submission
        self.db.execute(
            "INSERT INTO scribe_submissions (worker_name, content, status, created_at) VALUES (?1, ?2, 'pending', ?3)",
            params![worker_name, content, now],
        )?;
        let id = self.db.last_insert_rowid();

        // If this is the first pending submission, start the batch timer
        let pending_count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM scribe_submissions WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;

        if pending_count == 1 {
            // First pending - start the batch timer
            self.db.execute(
                "UPDATE state SET scribe_batch_started_at = ?1 WHERE id = 1",
                params![now],
            )?;
        }

        self.log_history(
            "scribe_submit",
            Some(&format!("Worker '{}' submitted learning", worker_name)),
        )?;

        Ok(id)
    }

    /// Get pending scribe submissions (including failed with retry_count < 3)
    pub fn get_pending_scribe_submissions(&self) -> StateResult<Vec<ScribeSubmission>> {
        let mut stmt = self.db.prepare(
            "SELECT id, worker_name, content, status, batch_id, retry_count, created_at, processed_at
             FROM scribe_submissions
             WHERE status = 'pending' OR (status = 'failed' AND retry_count < 3)
             ORDER BY id"
        )?;
        let submissions = stmt
            .query_map([], Self::submission_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(submissions)
    }

    /// Check if a batch is currently being processed
    pub fn get_processing_scribe_batch(&self) -> StateResult<Option<i64>> {
        let batch_id: Option<i64> = self.db.query_row(
            "SELECT DISTINCT batch_id FROM scribe_submissions WHERE status = 'processing' LIMIT 1",
            [],
            |row| row.get(0),
        ).ok();
        Ok(batch_id)
    }

    /// Mark submissions as processing with a batch ID
    pub fn mark_scribe_processing(&self, ids: &[i64], batch_id: i64) -> StateResult<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "UPDATE scribe_submissions SET status = 'processing', batch_id = ?1 WHERE id IN ({})",
            placeholders
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(batch_id)];
        for id in ids {
            params.push(Box::new(*id));
        }
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        self.db.execute(&sql, param_refs.as_slice())?;
        Ok(())
    }

    /// Complete a scribe batch - mark submissions as done or failed
    pub fn complete_scribe_batch(&self, batch_id: i64, success: bool) -> StateResult<()> {
        let now = self.now();

        if success {
            self.db.execute(
                "UPDATE scribe_submissions SET status = 'done', processed_at = ?1 WHERE batch_id = ?2 AND status = 'processing'",
                params![now, batch_id],
            )?;

            // Increment docs_version so workers know to re-sync
            self.db.execute(
                "UPDATE state SET docs_version = COALESCE(docs_version, 0) + 1 WHERE id = 1",
                [],
            )?;
        } else {
            self.db.execute(
                "UPDATE scribe_submissions SET status = 'failed', retry_count = retry_count + 1 WHERE batch_id = ?1 AND status = 'processing'",
                params![batch_id],
            )?;
        }

        // Clear the batch timer
        self.db.execute(
            "UPDATE state SET scribe_batch_started_at = NULL WHERE id = 1",
            [],
        )?;

        self.log_history(
            "scribe_batch",
            Some(&format!(
                "Batch {} {}",
                batch_id,
                if success { "completed" } else { "failed" }
            )),
        )?;

        Ok(())
    }

    /// Get when the current batch started (if any)
    pub fn get_scribe_batch_started_at(&self) -> StateResult<Option<String>> {
        let started_at: Option<String> = self
            .db
            .query_row(
                "SELECT scribe_batch_started_at FROM state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(started_at)
    }

    /// Get the current docs version (for sync)
    pub fn get_docs_version(&self) -> StateResult<i64> {
        let version: i64 = self
            .db
            .query_row(
                "SELECT COALESCE(docs_version, 0) FROM state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(version)
    }

    /// Generate the next batch ID
    pub fn next_scribe_batch_id(&self) -> StateResult<i64> {
        let max_id: Option<i64> = self
            .db
            .query_row("SELECT MAX(batch_id) FROM scribe_submissions", [], |row| {
                row.get(0)
            })
            .ok();
        Ok(max_id.unwrap_or(0) + 1)
    }
}
