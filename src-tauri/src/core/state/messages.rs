//! Message methods
//!
//! Methods for managing messages, threads, and notifications.

use chrono::{DateTime, Utc};
use sqlx::Row;

use super::types::{Message, RunStateSummary, StateResult, Status};
use super::SQLiteState;
use crate::core::db::utc_now;

impl SQLiteState {
    // =========================================================================
    // Message Methods
    // =========================================================================

    /// Add a message
    pub async fn add_message(
        &self,
        thread: &str,
        sender: &str,
        content: &str,
        waiting: bool,
    ) -> StateResult<i64> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "INSERT INTO messages (thread, sender, content, timestamp, waiting) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(thread)
        .bind(sender)
        .bind(content)
        .bind(utc_now())
        .bind(if waiting { 1i64 } else { 0i64 })
        .execute(&pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Get messages from a thread
    pub async fn get_messages(&self, thread: &str, limit: i64) -> StateResult<Vec<Message>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE thread = ? ORDER BY id LIMIT ?",
        )
        .bind(thread)
        .bind(limit)
        .fetch_all(&pool)
        .await?;

        let messages = rows
            .into_iter()
            .map(|row| Message {
                id: row.get("id"),
                thread: row.get("thread"),
                sender: row.get("sender"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                waiting: row.get::<i64, _>("waiting") != 0,
            })
            .collect();
        Ok(messages)
    }

    /// Get count of messages in a thread (avoids fetching all messages)
    pub async fn get_messages_count(&self, thread: &str) -> StateResult<i64> {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE thread = ?")
            .bind(thread)
            .fetch_one(&pool)
            .await?;
        Ok(count)
    }

    /// Get messages since a timestamp
    pub async fn get_messages_since(
        &self,
        since: &str,
        exclude_sender: Option<&str>,
    ) -> StateResult<Vec<Message>> {
        let pool = self.pool().await;
        let rows = if let Some(sender) = exclude_sender {
            sqlx::query(
                "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE timestamp > ? AND sender != ? ORDER BY id",
            )
            .bind(since)
            .bind(sender)
            .fetch_all(&pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE timestamp > ? ORDER BY id",
            )
            .bind(since)
            .fetch_all(&pool)
            .await?
        };

        let messages = rows
            .into_iter()
            .map(|row| Message {
                id: row.get("id"),
                thread: row.get("thread"),
                sender: row.get("sender"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                waiting: row.get::<i64, _>("waiting") != 0,
            })
            .collect();
        Ok(messages)
    }

    /// Get unread messages for a reader in a thread
    pub async fn get_unread_messages(
        &self,
        thread: &str,
        reader: &str,
    ) -> StateResult<Vec<Message>> {
        let pool = self.pool().await;
        let last_read_id: i64 = sqlx::query_scalar(
            "SELECT last_read_id FROM message_reads WHERE worker_name = ? AND thread = ?",
        )
        .bind(reader)
        .bind(thread)
        .fetch_optional(&pool)
        .await?
        .unwrap_or(0);

        let rows = sqlx::query(
            "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE thread = ? AND id > ? AND sender != ? ORDER BY id",
        )
        .bind(thread)
        .bind(last_read_id)
        .bind(reader)
        .fetch_all(&pool)
        .await?;

        let messages = rows
            .into_iter()
            .map(|row| Message {
                id: row.get("id"),
                thread: row.get("thread"),
                sender: row.get("sender"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                waiting: row.get::<i64, _>("waiting") != 0,
            })
            .collect();
        Ok(messages)
    }

    /// Get all unread messages for a reader
    pub async fn get_all_unread_messages(&self, reader: &str) -> StateResult<Vec<Message>> {
        let threads = self.get_threads().await?;
        let mut all_unread = vec![];
        for thread in threads {
            all_unread.extend(self.get_unread_messages(&thread, reader).await?);
        }
        Ok(all_unread)
    }

    /// Mark messages as read
    pub async fn mark_messages_read(
        &self,
        thread: &str,
        reader: &str,
        up_to_id: Option<i64>,
    ) -> StateResult<()> {
        let pool = self.pool().await;
        let max_id = match up_to_id {
            Some(id) => id,
            None => sqlx::query_scalar::<_, Option<i64>>(
                "SELECT MAX(id) FROM messages WHERE thread = ?",
            )
            .bind(thread)
            .fetch_one(&pool)
            .await?
            .unwrap_or(0),
        };

        sqlx::query(
            "INSERT INTO message_reads (worker_name, thread, last_read_id) VALUES (?, ?, ?) ON CONFLICT(worker_name, thread) DO UPDATE SET last_read_id = excluded.last_read_id",
        )
        .bind(reader)
        .bind(thread)
        .bind(max_id)
        .execute(&pool)
        .await?;
        Ok(())
    }

    /// Get all thread names
    pub async fn get_threads(&self) -> StateResult<Vec<String>> {
        let pool = self.pool().await;
        let threads: Vec<String> =
            sqlx::query_scalar("SELECT DISTINCT thread FROM messages ORDER BY thread")
                .fetch_all(&pool)
                .await?;
        Ok(threads)
    }

    /// Get message count for a thread
    pub async fn get_thread_message_count(&self, thread: &str) -> StateResult<i64> {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE thread = ?")
            .bind(thread)
            .fetch_one(&pool)
            .await?;
        Ok(count)
    }

    // =========================================================================
    // Notification Methods
    // =========================================================================

    /// Increment unread count
    pub async fn increment_unread(&self) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET unread_count = COALESCE(unread_count, 0) + 1 WHERE id = 1")
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Clear unread count
    pub async fn clear_unread(&self) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("UPDATE state SET unread_count = 0 WHERE id = 1")
            .execute(&pool)
            .await?;
        Ok(())
    }

    /// Get unread count
    pub async fn get_unread_count(&self) -> StateResult<i64> {
        let pool = self.pool().await;
        let result: Option<Option<i64>> =
            sqlx::query_scalar("SELECT unread_count FROM state WHERE id = 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result.flatten().unwrap_or(0))
    }

    /// Get all run summary data in an optimized single fetch
    pub async fn get_run_summary(&self) -> StateResult<RunStateSummary> {
        let pool = self.pool().await;

        // Query 1: Get all needed state columns in one query
        let state_row = sqlx::query(
            "SELECT status, created_at, updated_at, started_at, time_limit_minutes, COALESCE(unread_count, 0) as unread_count, worker_scale FROM state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await?;

        let status: String = state_row.get("status");
        let created_at: Option<String> = state_row.get("created_at");
        let updated_at: Option<String> = state_row.get("updated_at");
        let started_at: Option<String> = state_row.get("started_at");
        let time_limit_minutes: Option<i64> = state_row.get("time_limit_minutes");
        let unread_count: i64 = state_row.get("unread_count");
        let worker_scale: Option<String> = state_row.get("worker_scale");

        // Query 2: Get worker counts
        let worker_row = sqlx::query(
            "SELECT COUNT(*) as total, SUM(CASE WHEN status = 'working' THEN 1 ELSE 0 END) as active FROM workers",
        )
        .fetch_one(&pool)
        .await?;

        let workers_registered: i64 = worker_row.get("total");
        let workers_active: Option<i64> = worker_row.get("active");

        // workers_total is the max scale (from worker_scale), falling back to registered count
        let workers_total = worker_scale
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(workers_registered as u32);

        // Parse status
        let status = Status::from_str(&status).unwrap_or(Status::Draft);

        // Calculate elapsed minutes based on status
        let elapsed_minutes = if status == Status::Draft {
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

        Ok(RunStateSummary {
            status,
            created_at,
            updated_at,
            started_at,
            time_limit_minutes,
            unread_count,
            workers_active: workers_active.unwrap_or(0) as u32,
            workers_total,
            elapsed_minutes,
        })
    }
}
