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
-- run_name is nullable: NULL means conversation without a run selected
CREATE TABLE IF NOT EXISTS gyp_chat_messages (
    id INTEGER PRIMARY KEY,
    run_name TEXT,            -- NULL for no-run conversations
    role TEXT NOT NULL,       -- 'user', 'assistant', 'system'
    timestamp TEXT NOT NULL,
    chunks_json TEXT NOT NULL -- JSON-encoded message chunks
);

CREATE INDEX IF NOT EXISTS idx_gyp_chat_run ON gyp_chat_messages(run_name);
CREATE INDEX IF NOT EXISTS idx_gyp_chat_timestamp ON gyp_chat_messages(timestamp);
"#;

/// Gyp chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GypChatMessage {
    pub id: i64,
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
        self.db.execute(
            "INSERT INTO gyp_chat_messages (run_name, role, timestamp, chunks_json) VALUES (?1, ?2, ?3, ?4)",
            params![run_name, role, self.now(), chunks_json],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get all messages for a run (or no-run if run_name is None)
    pub fn get_messages(&self, run_name: Option<&str>) -> GypChatResult<Vec<GypChatMessage>> {
        let mut stmt = if run_name.is_some() {
            self.db.prepare(
                "SELECT id, run_name, role, timestamp, chunks_json
                 FROM gyp_chat_messages
                 WHERE run_name = ?1
                 ORDER BY timestamp ASC",
            )?
        } else {
            self.db.prepare(
                "SELECT id, run_name, role, timestamp, chunks_json
                 FROM gyp_chat_messages
                 WHERE run_name IS NULL
                 ORDER BY timestamp ASC",
            )?
        };

        let messages = if run_name.is_some() {
            stmt.query_map([run_name], |row| {
                Ok(GypChatMessage {
                    id: row.get("id")?,
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

    /// Clear all messages for a run (or no-run if run_name is None)
    pub fn clear_messages(&self, run_name: Option<&str>) -> GypChatResult<()> {
        if run_name.is_some() {
            self.db.execute(
                "DELETE FROM gyp_chat_messages WHERE run_name = ?1",
                params![run_name],
            )?;
        } else {
            self.db
                .execute("DELETE FROM gyp_chat_messages WHERE run_name IS NULL", [])?;
        }
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

        // Get messages for no run
        let no_run_msgs = store.get_messages(None).unwrap();
        assert_eq!(no_run_msgs.len(), 1);

        // Clear run messages
        store.clear_messages(Some("test-run")).unwrap();
        let run_msgs = store.get_messages(Some("test-run")).unwrap();
        assert_eq!(run_msgs.len(), 0);

        // No-run messages should still exist
        let no_run_msgs = store.get_messages(None).unwrap();
        assert_eq!(no_run_msgs.len(), 1);
    }
}
