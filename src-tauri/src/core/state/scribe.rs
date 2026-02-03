//! Scribe submission methods
//!
//! Methods for managing scribe submissions - learnings recorded by workers
//! that are batched and processed by an ephemeral Scribe agent.

use sqlx::Row;

use super::types::StateResult;
use super::SQLiteState;
use crate::core::db::utc_now;

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

    /// Add a scribe submission from a worker
    ///
    /// If this is the first pending submission, also sets scribe_batch_started_at
    pub async fn add_scribe_submission(
        &self,
        worker_name: &str,
        content: &str,
    ) -> StateResult<i64> {
        let pool = self.pool().await;
        let now = utc_now();

        // Insert the submission
        let result = sqlx::query(
            "INSERT INTO scribe_submissions (worker_name, content, status, created_at) VALUES (?, ?, 'pending', ?)",
        )
        .bind(worker_name)
        .bind(content)
        .bind(&now)
        .execute(&pool)
        .await?;
        let id = result.last_insert_rowid();

        // If this is the first pending submission, start the batch timer
        let pending_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM scribe_submissions WHERE status = 'pending'")
                .fetch_one(&pool)
                .await?;

        if pending_count == 1 {
            // First pending - start the batch timer
            sqlx::query("UPDATE state SET scribe_batch_started_at = ? WHERE id = 1")
                .bind(&now)
                .execute(&pool)
                .await?;
        }

        self.log_history(
            "scribe_submit",
            Some(&format!("Worker '{}' submitted learning", worker_name)),
        )
        .await?;

        Ok(id)
    }

    /// Get pending scribe submissions (including failed with retry_count < 3)
    pub async fn get_pending_scribe_submissions(&self) -> StateResult<Vec<ScribeSubmission>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, worker_name, content, status, batch_id, retry_count, created_at, processed_at
             FROM scribe_submissions
             WHERE status = 'pending' OR (status = 'failed' AND retry_count < 3)
             ORDER BY id",
        )
        .fetch_all(&pool)
        .await?;

        let submissions = rows
            .into_iter()
            .map(|row| ScribeSubmission {
                id: row.get("id"),
                worker_name: row.get("worker_name"),
                content: row.get("content"),
                status: row.get("status"),
                batch_id: row.get("batch_id"),
                retry_count: row.get("retry_count"),
                created_at: row.get("created_at"),
                processed_at: row.get("processed_at"),
            })
            .collect();
        Ok(submissions)
    }

    /// Check if a batch is currently being processed
    pub async fn get_processing_scribe_batch(&self) -> StateResult<Option<i64>> {
        let pool = self.pool().await;
        let batch_id: Option<i64> = sqlx::query_scalar(
            "SELECT DISTINCT batch_id FROM scribe_submissions WHERE status = 'processing' LIMIT 1",
        )
        .fetch_optional(&pool)
        .await?;
        Ok(batch_id)
    }

    /// Mark submissions as processing with a batch ID
    pub async fn mark_scribe_processing(&self, ids: &[i64], batch_id: i64) -> StateResult<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let pool = self.pool().await;
        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "UPDATE scribe_submissions SET status = 'processing', batch_id = ? WHERE id IN ({})",
            placeholders
        );

        let mut query = sqlx::query(&sql).bind(batch_id);
        for id in ids {
            query = query.bind(id);
        }
        query.execute(&pool).await?;
        Ok(())
    }

    /// Complete a scribe batch - mark submissions as done or failed
    pub async fn complete_scribe_batch(&self, batch_id: i64, success: bool) -> StateResult<()> {
        let pool = self.pool().await;
        let now = utc_now();

        if success {
            sqlx::query(
                "UPDATE scribe_submissions SET status = 'done', processed_at = ? WHERE batch_id = ? AND status = 'processing'",
            )
            .bind(&now)
            .bind(batch_id)
            .execute(&pool)
            .await?;

            // Increment docs_version so workers know to re-sync
            sqlx::query(
                "UPDATE state SET docs_version = COALESCE(docs_version, 0) + 1 WHERE id = 1",
            )
            .execute(&pool)
            .await?;
        } else {
            sqlx::query(
                "UPDATE scribe_submissions SET status = 'failed', retry_count = retry_count + 1 WHERE batch_id = ? AND status = 'processing'",
            )
            .bind(batch_id)
            .execute(&pool)
            .await?;
        }

        // Clear the batch timer
        sqlx::query("UPDATE state SET scribe_batch_started_at = NULL WHERE id = 1")
            .execute(&pool)
            .await?;

        self.log_history(
            "scribe_batch",
            Some(&format!(
                "Batch {} {}",
                batch_id,
                if success { "completed" } else { "failed" }
            )),
        )
        .await?;

        Ok(())
    }

    /// Get when the current batch started (if any)
    pub async fn get_scribe_batch_started_at(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let started_at: Option<Option<String>> =
            sqlx::query_scalar("SELECT scribe_batch_started_at FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(started_at.flatten())
    }

    /// Get the current docs version (for sync)
    pub async fn get_docs_version(&self) -> StateResult<i64> {
        let pool = self.pool().await;
        let version: i64 =
            sqlx::query_scalar("SELECT COALESCE(docs_version, 0) FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?
                .unwrap_or(0);
        Ok(version)
    }

    /// Generate the next batch ID
    pub async fn next_scribe_batch_id(&self) -> StateResult<i64> {
        let pool = self.pool().await;
        // MAX returns NULL if no rows, which becomes None
        // fetch_optional wraps that in another Option
        let max_id: Option<Option<i64>> =
            sqlx::query_scalar("SELECT MAX(batch_id) FROM scribe_submissions")
                .fetch_optional(&pool)
                .await?;
        Ok(max_id.flatten().unwrap_or(0) + 1)
    }
}
