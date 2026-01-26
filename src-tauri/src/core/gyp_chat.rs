//! Gyp Chat History Storage
//!
//! Stores Gyp (AI assistant) chat history in the global hirsel database.
//! Chat history is associated with run names to maintain separate conversations per run.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::config::global_db_path;

/// Schema for Gyp chat tables
const SCHEMA: &str = r#"
-- Gyp chat messages for persistent per-run AI assistant history
-- Supports three scopes:
-- 1. project_id=NULL, run_name=NULL → general chat
-- 2. project_id=X, run_name=NULL → project-level chat
-- 3. project_id=X, run_name=Y → run-specific chat
CREATE TABLE IF NOT EXISTS gyp_chat_messages (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,       -- NULL for general conversations
    run_name TEXT,            -- NULL for project-level or general conversations
    role TEXT NOT NULL,       -- 'user', 'assistant', 'system'
    timestamp TEXT NOT NULL,
    chunks_json TEXT NOT NULL -- JSON-encoded message chunks
);

CREATE INDEX IF NOT EXISTS idx_gyp_chat_run ON gyp_chat_messages(run_name);
CREATE INDEX IF NOT EXISTS idx_gyp_chat_project ON gyp_chat_messages(project_id);
CREATE INDEX IF NOT EXISTS idx_gyp_chat_timestamp ON gyp_chat_messages(timestamp);
"#;

/// Gyp chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GypChatMessage {
    pub id: i64,
    pub project_id: Option<i64>,
    pub run_name: Option<String>,
    pub role: String,
    pub timestamp: String,
    pub chunks_json: String,
}

/// Error type for Gyp chat operations
#[derive(Debug, thiserror::Error)]
pub enum GypChatError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type GypChatResult<T> = Result<T, GypChatError>;

/// Gyp chat storage backed by SQLite
pub struct GypChatStore {
    db: Connection,
}

impl GypChatStore {
    /// Open the global Gyp chat store
    pub fn open() -> GypChatResult<Self> {
        Self::open_at(&global_db_path())
    }

    /// Open from a specific path (useful for testing)
    pub fn open_at(path: &Path) -> GypChatResult<Self> {
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

    fn init_db(&self) -> GypChatResult<()> {
        self.db.execute_batch(SCHEMA)?;
        Ok(())
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6f")
            .to_string()
    }

    /// Save a chat message
    pub fn save_message(
        &self,
        run_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> GypChatResult<i64> {
        self.save_message_with_project(None, run_name, role, chunks_json)
    }

    /// Save a chat message with project context
    pub fn save_message_with_project(
        &self,
        project_id: Option<i64>,
        run_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> GypChatResult<i64> {
        self.db.execute(
            "INSERT INTO gyp_chat_messages (project_id, run_name, role, timestamp, chunks_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project_id, run_name, role, self.now(), chunks_json],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get all messages for a run (or no-run if run_name is None)
    pub fn get_messages(&self, run_name: Option<&str>) -> GypChatResult<Vec<GypChatMessage>> {
        let mut stmt = if run_name.is_some() {
            self.db.prepare(
                "SELECT id, project_id, run_name, role, timestamp, chunks_json
                 FROM gyp_chat_messages
                 WHERE run_name = ?1
                 ORDER BY timestamp ASC",
            )?
        } else {
            self.db.prepare(
                "SELECT id, project_id, run_name, role, timestamp, chunks_json
                 FROM gyp_chat_messages
                 WHERE run_name IS NULL AND project_id IS NULL
                 ORDER BY timestamp ASC",
            )?
        };

        let messages = if run_name.is_some() {
            stmt.query_map([run_name], |row| {
                Ok(GypChatMessage {
                    id: row.get("id")?,
                    project_id: row.get("project_id")?,
                    run_name: row.get("run_name")?,
                    role: row.get("role")?,
                    timestamp: row.get("timestamp")?,
                    chunks_json: row.get("chunks_json")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], |row| {
                Ok(GypChatMessage {
                    id: row.get("id")?,
                    project_id: row.get("project_id")?,
                    run_name: row.get("run_name")?,
                    role: row.get("role")?,
                    timestamp: row.get("timestamp")?,
                    chunks_json: row.get("chunks_json")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        Ok(messages)
    }

    /// Get messages for a project (all runs or no run)
    pub fn get_project_messages(&self, project_id: i64) -> GypChatResult<Vec<GypChatMessage>> {
        let mut stmt = self.db.prepare(
            "SELECT id, project_id, run_name, role, timestamp, chunks_json
             FROM gyp_chat_messages
             WHERE project_id = ?1
             ORDER BY timestamp ASC",
        )?;

        let messages = stmt
            .query_map([project_id], |row| {
                Ok(GypChatMessage {
                    id: row.get("id")?,
                    project_id: row.get("project_id")?,
                    run_name: row.get("run_name")?,
                    role: row.get("role")?,
                    timestamp: row.get("timestamp")?,
                    chunks_json: row.get("chunks_json")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Clear all messages for a run (or no-run if run_name is None)
    pub fn clear_messages(&self, run_name: Option<&str>) -> GypChatResult<()> {
        if run_name.is_some() {
            self.db.execute(
                "DELETE FROM gyp_chat_messages WHERE run_name = ?1",
                params![run_name],
            )?;
        } else {
            self.db.execute(
                "DELETE FROM gyp_chat_messages WHERE run_name IS NULL AND project_id IS NULL",
                [],
            )?;
        }
        Ok(())
    }

    /// Clear all messages for a project
    pub fn clear_project_messages(&self, project_id: i64) -> GypChatResult<()> {
        self.db.execute(
            "DELETE FROM gyp_chat_messages WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }

    /// Delete all messages for a specific run (used when deleting a run)
    pub fn delete_run_messages(&self, run_name: &str) -> GypChatResult<()> {
        self.db.execute(
            "DELETE FROM gyp_chat_messages WHERE run_name = ?1",
            params![run_name],
        )?;
        Ok(())
    }

    // ========== BOARD CHAT METHODS ==========
    // Board chat uses a special run_name sentinel: "__board__"
    // This allows board chat history to be stored separately from run-specific chats.

    /// Save a board chat message for a project
    pub fn save_board_message(
        &self,
        project_id: i64,
        role: &str,
        chunks_json: &str,
    ) -> GypChatResult<i64> {
        self.save_message_with_project(Some(project_id), Some("__board__"), role, chunks_json)
    }

    /// Get board chat messages for a project (most recent first)
    pub fn get_board_messages(
        &self,
        project_id: i64,
        limit: usize,
    ) -> GypChatResult<Vec<GypChatMessage>> {
        let mut stmt = self.db.prepare(
            "SELECT id, project_id, run_name, role, timestamp, chunks_json
             FROM gyp_chat_messages
             WHERE project_id = ?1 AND run_name = '__board__'
             ORDER BY timestamp DESC
             LIMIT ?2",
        )?;

        let messages: Vec<GypChatMessage> = stmt
            .query_map(params![project_id, limit as i64], |row| {
                Ok(GypChatMessage {
                    id: row.get("id")?,
                    project_id: row.get("project_id")?,
                    run_name: row.get("run_name")?,
                    role: row.get("role")?,
                    timestamp: row.get("timestamp")?,
                    chunks_json: row.get("chunks_json")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Reverse to get chronological order (oldest first)
        Ok(messages.into_iter().rev().collect())
    }

    /// Clear all board chat messages for a project
    pub fn clear_board_messages(&self, project_id: i64) -> GypChatResult<()> {
        self.db.execute(
            "DELETE FROM gyp_chat_messages WHERE project_id = ?1 AND run_name = '__board__'",
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
    fn test_save_and_get_messages() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = GypChatStore::open_at(&db_path).unwrap();

        // Save messages for a run
        store
            .save_message(
                Some("test-run"),
                "user",
                r#"[{"type":"text","content":"hello"}]"#,
            )
            .unwrap();
        store
            .save_message(
                Some("test-run"),
                "assistant",
                r#"[{"type":"text","content":"hi there"}]"#,
            )
            .unwrap();

        // Save message for no run
        store
            .save_message(None, "user", r#"[{"type":"text","content":"no run"}]"#)
            .unwrap();

        // Get messages for run
        let run_msgs = store.get_messages(Some("test-run")).unwrap();
        assert_eq!(run_msgs.len(), 2);
        assert_eq!(run_msgs[0].role, "user");
        assert_eq!(run_msgs[1].role, "assistant");
        assert_eq!(run_msgs[0].project_id, None);

        // Get messages for no run
        let no_run_msgs = store.get_messages(None).unwrap();
        assert_eq!(no_run_msgs.len(), 1);
        assert_eq!(no_run_msgs[0].project_id, None);

        // Clear run messages
        store.clear_messages(Some("test-run")).unwrap();
        let run_msgs = store.get_messages(Some("test-run")).unwrap();
        assert_eq!(run_msgs.len(), 0);

        // No-run messages should still exist
        let no_run_msgs = store.get_messages(None).unwrap();
        assert_eq!(no_run_msgs.len(), 1);
    }
}
