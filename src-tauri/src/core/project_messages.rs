//! Project Messages Storage (Sheepfold)
//!
//! Stores project-scoped messaging in the global hirsel database.
//! Supports:
//! - Meadow (group chat with all workers + human)
//! - Worker DMs (direct messages via worker name threads)

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::config::global_db_path;

/// Schema for project messages tables
const SCHEMA: &str = r#"
-- Project-scoped messages for Sheepfold
-- thread = 'meadow' for group chat, or worker_name for DMs
CREATE TABLE IF NOT EXISTS project_messages (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    thread TEXT NOT NULL,
    sender TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    waiting INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_project_messages_project ON project_messages(project_id);
CREATE INDEX IF NOT EXISTS idx_project_messages_thread ON project_messages(project_id, thread);
CREATE INDEX IF NOT EXISTS idx_project_messages_timestamp ON project_messages(timestamp);

-- Read tracking for unread counts
CREATE TABLE IF NOT EXISTS project_message_reads (
    reader TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    thread TEXT NOT NULL,
    last_read_id INTEGER NOT NULL,
    PRIMARY KEY (reader, project_id, thread)
);
"#;

/// Project message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMessage {
    pub id: i64,
    pub project_id: i64,
    pub thread: String,
    pub sender: String,
    pub content: String,
    pub waiting: bool,
    pub timestamp: String,
}

/// Thread summary with unread count
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectThreadSummary {
    pub thread: String,
    pub message_count: i64,
    pub unread_count: i64,
    pub last_message: Option<String>,
    pub last_timestamp: Option<String>,
}

/// Error type for project message operations
#[derive(Debug, thiserror::Error)]
pub enum ProjectMessagesError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ProjectMessagesResult<T> = Result<T, ProjectMessagesError>;

/// Project messages storage backed by SQLite
pub struct ProjectMessagesStore {
    db: Connection,
}

impl ProjectMessagesStore {
    /// Open the global project messages store
    pub fn open() -> ProjectMessagesResult<Self> {
        Self::open_at(&global_db_path())
    }

    /// Open from a specific path (useful for testing)
    pub fn open_at(path: &Path) -> ProjectMessagesResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        // Enable WAL mode for better concurrent read/write performance
        db.pragma_update(None, "journal_mode", "WAL")?;

        let store = Self { db };
        store.init_db()?;
        Ok(store)
    }

    fn init_db(&self) -> ProjectMessagesResult<()> {
        self.db.execute_batch(SCHEMA)?;
        Ok(())
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6f")
            .to_string()
    }

    /// Add a message to a thread
    pub fn add_message(
        &self,
        project_id: i64,
        thread: &str,
        sender: &str,
        content: &str,
        waiting: bool,
    ) -> ProjectMessagesResult<ProjectMessage> {
        let timestamp = self.now();
        let waiting_int = if waiting { 1 } else { 0 };

        self.db.execute(
            "INSERT INTO project_messages (project_id, thread, sender, content, timestamp, waiting)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![project_id, thread, sender, content, timestamp, waiting_int],
        )?;

        let id = self.db.last_insert_rowid();

        Ok(ProjectMessage {
            id,
            project_id,
            thread: thread.to_string(),
            sender: sender.to_string(),
            content: content.to_string(),
            waiting,
            timestamp,
        })
    }

    /// Get messages for a thread
    pub fn get_messages(
        &self,
        project_id: i64,
        thread: &str,
        limit: Option<i64>,
    ) -> ProjectMessagesResult<Vec<ProjectMessage>> {
        let limit = limit.unwrap_or(100);

        let mut stmt = self.db.prepare(
            "SELECT id, project_id, thread, sender, content, timestamp, waiting
             FROM project_messages
             WHERE project_id = ?1 AND thread = ?2
             ORDER BY timestamp DESC
             LIMIT ?3",
        )?;

        let messages: Vec<ProjectMessage> = stmt
            .query_map(params![project_id, thread, limit], |row| {
                Ok(ProjectMessage {
                    id: row.get("id")?,
                    project_id: row.get("project_id")?,
                    thread: row.get("thread")?,
                    sender: row.get("sender")?,
                    content: row.get("content")?,
                    timestamp: row.get("timestamp")?,
                    waiting: row.get::<_, i64>("waiting")? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Reverse to get chronological order (oldest first)
        Ok(messages.into_iter().rev().collect())
    }

    /// Get all threads for a project with unread counts
    pub fn get_threads(
        &self,
        project_id: i64,
        reader: &str,
    ) -> ProjectMessagesResult<Vec<ProjectThreadSummary>> {
        let mut stmt = self.db.prepare(
            r#"
            SELECT
                m.thread,
                COUNT(*) as message_count,
                (SELECT content FROM project_messages
                 WHERE project_id = m.project_id AND thread = m.thread
                 ORDER BY timestamp DESC LIMIT 1) as last_message,
                (SELECT timestamp FROM project_messages
                 WHERE project_id = m.project_id AND thread = m.thread
                 ORDER BY timestamp DESC LIMIT 1) as last_timestamp,
                COALESCE(
                    (SELECT COUNT(*) FROM project_messages pm
                     WHERE pm.project_id = m.project_id
                       AND pm.thread = m.thread
                       AND pm.id > COALESCE(
                           (SELECT last_read_id FROM project_message_reads
                            WHERE reader = ?2 AND project_id = m.project_id AND thread = m.thread),
                           0
                       )
                    ),
                    0
                ) as unread_count
            FROM project_messages m
            WHERE m.project_id = ?1
            GROUP BY m.thread
            ORDER BY last_timestamp DESC
            "#,
        )?;

        let threads = stmt
            .query_map(params![project_id, reader], |row| {
                Ok(ProjectThreadSummary {
                    thread: row.get("thread")?,
                    message_count: row.get("message_count")?,
                    unread_count: row.get("unread_count")?,
                    last_message: row.get("last_message")?,
                    last_timestamp: row.get("last_timestamp")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(threads)
    }

    /// Mark messages as read up to the latest message
    pub fn mark_messages_read(
        &self,
        project_id: i64,
        thread: &str,
        reader: &str,
    ) -> ProjectMessagesResult<()> {
        // Get the latest message ID
        let last_id: Option<i64> = self.db.query_row(
            "SELECT MAX(id) FROM project_messages WHERE project_id = ?1 AND thread = ?2",
            params![project_id, thread],
            |row| row.get(0),
        )?;

        if let Some(id) = last_id {
            self.db.execute(
                "INSERT OR REPLACE INTO project_message_reads (reader, project_id, thread, last_read_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![reader, project_id, thread, id],
            )?;
        }

        Ok(())
    }

    /// Get total unread count across all threads for a project
    pub fn get_unread_count(&self, project_id: i64, reader: &str) -> ProjectMessagesResult<i64> {
        let count: i64 = self.db.query_row(
            r#"
            SELECT COALESCE(SUM(
                (SELECT COUNT(*) FROM project_messages pm
                 WHERE pm.project_id = ?1
                   AND pm.thread = threads.thread
                   AND pm.id > COALESCE(
                       (SELECT last_read_id FROM project_message_reads
                        WHERE reader = ?2 AND project_id = ?1 AND thread = threads.thread),
                       0
                   )
                )
            ), 0)
            FROM (SELECT DISTINCT thread FROM project_messages WHERE project_id = ?1) threads
            "#,
            params![project_id, reader],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    /// Delete all messages for a project (called when project is deleted)
    pub fn delete_project_messages(&self, project_id: i64) -> ProjectMessagesResult<()> {
        self.db.execute(
            "DELETE FROM project_messages WHERE project_id = ?1",
            params![project_id],
        )?;
        self.db.execute(
            "DELETE FROM project_message_reads WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_add_and_get_messages() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ProjectMessagesStore::open_at(&db_path).unwrap();

        // Add messages to meadow
        store
            .add_message(1, "meadow", "user", "Hello everyone!", false)
            .unwrap();
        store
            .add_message(1, "meadow", "willow", "Hi there!", false)
            .unwrap();

        // Add message to worker DM
        store
            .add_message(1, "willow", "willow", "Direct message", false)
            .unwrap();

        // Get meadow messages
        let meadow_msgs = store.get_messages(1, "meadow", None).unwrap();
        assert_eq!(meadow_msgs.len(), 2);
        assert_eq!(meadow_msgs[0].sender, "user");
        assert_eq!(meadow_msgs[1].sender, "willow");

        // Get DM messages
        let dm_msgs = store.get_messages(1, "willow", None).unwrap();
        assert_eq!(dm_msgs.len(), 1);
    }

    #[test]
    fn test_threads_and_unread() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ProjectMessagesStore::open_at(&db_path).unwrap();

        // Add messages
        store
            .add_message(1, "meadow", "user", "Hello", false)
            .unwrap();
        store
            .add_message(1, "meadow", "willow", "Hi", false)
            .unwrap();
        store
            .add_message(1, "bramble", "bramble", "DM here", false)
            .unwrap();

        // Get threads
        let threads = store.get_threads(1, "user").unwrap();
        assert_eq!(threads.len(), 2);

        // Check unread counts (all unread for user)
        let total_unread = store.get_unread_count(1, "user").unwrap();
        assert_eq!(total_unread, 3);

        // Mark meadow as read
        store.mark_messages_read(1, "meadow", "user").unwrap();

        // Check unread again
        let total_unread = store.get_unread_count(1, "user").unwrap();
        assert_eq!(total_unread, 1); // Only bramble DM unread
    }
}
