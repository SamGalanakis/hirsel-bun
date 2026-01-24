//! Message methods
//!
//! Methods for managing messages, threads, and notifications.

use chrono::{DateTime, Utc};
use rusqlite::{params, Row};

use super::types::{Message, RunStateSummary, StateError, StateResult, Status};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // Message Methods
    // =========================================================================

    pub(super) fn message_from_row(row: &Row) -> rusqlite::Result<Message> {
        Ok(Message {
            id: row.get("id")?,
            thread: row.get("thread")?,
            sender: row.get("sender")?,
            content: row.get("content")?,
            timestamp: row.get("timestamp")?,
            waiting: row.get::<_, i64>("waiting")? != 0,
        })
    }

    /// Add a message
    pub fn add_message(
        &self,
        thread: &str,
        sender: &str,
        content: &str,
        waiting: bool,
    ) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO messages (thread, sender, content, timestamp, waiting) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![thread, sender, content, self.now(), if waiting { 1 } else { 0 }],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get messages from a thread
    pub fn get_messages(&self, thread: &str, limit: i64) -> StateResult<Vec<Message>> {
        let mut stmt = self.db.prepare(
            "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE thread = ?1 ORDER BY id LIMIT ?2"
        )?;
        let messages = stmt
            .query_map(params![thread, limit], Self::message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// Get count of messages in a thread (avoids fetching all messages)
    pub fn get_messages_count(&self, thread: &str) -> StateResult<i64> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM messages WHERE thread = ?1",
            params![thread],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get messages since a timestamp
    pub fn get_messages_since(
        &self,
        since: &str,
        exclude_sender: Option<&str>,
    ) -> StateResult<Vec<Message>> {
        if let Some(sender) = exclude_sender {
            let mut stmt = self.db.prepare(
                "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE timestamp > ?1 AND sender != ?2 ORDER BY id"
            )?;
            let messages = stmt
                .query_map(params![since, sender], Self::message_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        } else {
            let mut stmt = self.db.prepare(
                "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE timestamp > ?1 ORDER BY id"
            )?;
            let messages = stmt
                .query_map(params![since], Self::message_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(messages)
        }
    }

    /// Get unread messages for a reader in a thread
    pub fn get_unread_messages(&self, thread: &str, reader: &str) -> StateResult<Vec<Message>> {
        let last_read_id: i64 = self
            .db
            .query_row(
                "SELECT last_read_id FROM message_reads WHERE worker_name = ?1 AND thread = ?2",
                params![reader, thread],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let mut stmt = self.db.prepare(
            "SELECT id, thread, sender, content, timestamp, waiting FROM messages WHERE thread = ?1 AND id > ?2 AND sender != ?3 ORDER BY id"
        )?;
        let messages = stmt
            .query_map(
                params![thread, last_read_id, reader],
                Self::message_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// Get all unread messages for a reader
    pub fn get_all_unread_messages(&self, reader: &str) -> StateResult<Vec<Message>> {
        let threads = self.get_threads()?;
        let mut all_unread = vec![];
        for thread in threads {
            all_unread.extend(self.get_unread_messages(&thread, reader)?);
        }
        Ok(all_unread)
    }

    /// Mark messages as read
    pub fn mark_messages_read(
        &self,
        thread: &str,
        reader: &str,
        up_to_id: Option<i64>,
    ) -> StateResult<()> {
        let max_id = match up_to_id {
            Some(id) => id,
            None => self
                .db
                .query_row(
                    "SELECT MAX(id) FROM messages WHERE thread = ?1",
                    params![thread],
                    |row| row.get::<_, Option<i64>>(0),
                )?
                .unwrap_or(0),
        };

        self.db.execute(
            "INSERT INTO message_reads (worker_name, thread, last_read_id) VALUES (?1, ?2, ?3) ON CONFLICT(worker_name, thread) DO UPDATE SET last_read_id = ?3",
            params![reader, thread, max_id],
        )?;
        Ok(())
    }

    /// Get all thread names
    pub fn get_threads(&self) -> StateResult<Vec<String>> {
        let mut stmt = self
            .db
            .prepare("SELECT DISTINCT thread FROM messages ORDER BY thread")?;
        let threads: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(threads)
    }

    /// Get message count for a thread
    pub fn get_thread_message_count(&self, thread: &str) -> StateResult<i64> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM messages WHERE thread = ?1",
            params![thread],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // =========================================================================
    // Notification Methods
    // =========================================================================

    /// Increment unread count
    pub fn increment_unread(&self) -> StateResult<()> {
        self.db.execute(
            "UPDATE state SET unread_count = COALESCE(unread_count, 0) + 1 WHERE id = 1",
            [],
        )?;
        Ok(())
    }

    /// Clear unread count
    pub fn clear_unread(&self) -> StateResult<()> {
        self.db
            .execute("UPDATE state SET unread_count = 0 WHERE id = 1", [])?;
        Ok(())
    }

    /// Get unread count
    pub fn get_unread_count(&self) -> StateResult<i64> {
        match self
            .db
            .query_row("SELECT unread_count FROM state WHERE id = 1", [], |row| {
                row.get::<_, Option<i64>>(0)
            }) {
            Ok(Some(val)) => Ok(val),
            Ok(None) => Ok(0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get all run summary data in an optimized single fetch
    /// This fetches state, task counts, and worker counts in 3 queries instead of ~9
    pub fn get_run_summary(&self) -> StateResult<RunStateSummary> {
        // Query 1: Get all needed state columns in one query (including worker_scale for max workers)
        let (status, created_at, updated_at, started_at, time_limit_minutes, unread_count, worker_scale): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            i64,
            Option<String>,
        ) = self.db.query_row(
            "SELECT status, created_at, updated_at, started_at, time_limit_minutes, COALESCE(unread_count, 0), worker_scale FROM state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )?;

        // Query 2: Get task counts in one aggregate query
        let (tasks_total, tasks_done): (u32, u32) = self.db.query_row(
            "SELECT COUNT(*), SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) FROM tasks",
            [],
            |row| {
                let total: i64 = row.get(0)?;
                let done: Option<i64> = row.get(1)?;
                Ok((total as u32, done.unwrap_or(0) as u32))
            },
        )?;

        // Query 3: Get worker counts - active workers from workers table
        let (workers_registered, workers_active): (u32, u32) = self.db.query_row(
            "SELECT COUNT(*), SUM(CASE WHEN status = 'working' THEN 1 ELSE 0 END) FROM workers",
            [],
            |row| {
                let total: i64 = row.get(0)?;
                let active: Option<i64> = row.get(1)?;
                Ok((total as u32, active.unwrap_or(0) as u32))
            },
        )?;

        // workers_total is the max scale (from worker_scale), falling back to registered count
        let workers_total = worker_scale
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(workers_registered);

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
            tasks_done,
            tasks_total,
            workers_active,
            workers_total,
            elapsed_minutes,
        })
    }

    // =========================================================================
    // Compaction Methods
    // =========================================================================

    /// Compact messages in a thread
    pub fn compact_messages(
        &self,
        thread: &str,
        ids_to_delete: &[i64],
        summary_content: &str,
    ) -> StateResult<()> {
        if ids_to_delete.is_empty() {
            return Ok(());
        }

        // Delete old messages
        let placeholders: String = ids_to_delete
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("DELETE FROM messages WHERE id IN ({})", placeholders);
        let params: Vec<&dyn rusqlite::ToSql> = ids_to_delete
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        self.db.execute(&sql, params.as_slice())?;

        // Insert summary message
        self.db.execute(
            "INSERT INTO messages (thread, sender, content, timestamp, waiting) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![thread, "hirsel", summary_content, self.now(), 0],
        )?;

        // Reset last_read_id for all workers on this thread
        self.db.execute(
            "DELETE FROM message_reads WHERE thread = ?1",
            params![thread],
        )?;

        self.log_history(
            "compaction",
            Some(&format!(
                "Compacted {} messages in thread '{}'",
                ids_to_delete.len(),
                thread
            )),
        )?;

        Ok(())
    }
}
