//! Gyp Chat History Storage
//!
//! Stores Gyp (AI assistant) chat history in the global hirsel database.
//! Chat history is associated with run names to maintain separate conversations per run.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::db::{global_pool, utc_now};

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

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    SCHEMA_INIT
        .get_or_try_init(|| async {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
            Ok::<(), sqlx::Error>(())
        })
        .await?;
    Ok(())
}

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
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type GypChatResult<T> = Result<T, GypChatError>;

/// Gyp chat storage backed by SQLite
pub struct GypChatStore;

impl GypChatStore {
    /// Open the global Gyp chat store
    pub async fn open() -> GypChatResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    /// Get the pool
    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    /// Save a chat message
    pub async fn save_message(
        &self,
        run_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> GypChatResult<i64> {
        self.save_message_with_project(None, run_name, role, chunks_json)
            .await
    }

    /// Save a chat message with project context
    pub async fn save_message_with_project(
        &self,
        project_id: Option<i64>,
        run_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> GypChatResult<i64> {
        let pool = self.pool().await;
        let timestamp = utc_now();

        let result = sqlx::query(
            "INSERT INTO gyp_chat_messages (project_id, run_name, role, timestamp, chunks_json) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(run_name)
        .bind(role)
        .bind(&timestamp)
        .bind(chunks_json)
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get all messages for a run (or no-run if run_name is None)
    pub async fn get_messages(&self, run_name: Option<&str>) -> GypChatResult<Vec<GypChatMessage>> {
        let pool = self.pool().await;

        let rows = if run_name.is_some() {
            sqlx::query(
                "SELECT id, project_id, run_name, role, timestamp, chunks_json
                 FROM gyp_chat_messages
                 WHERE run_name = ?
                 ORDER BY timestamp ASC",
            )
            .bind(run_name)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, project_id, run_name, role, timestamp, chunks_json
                 FROM gyp_chat_messages
                 WHERE run_name IS NULL AND project_id IS NULL
                 ORDER BY timestamp ASC",
            )
            .fetch_all(pool)
            .await?
        };

        let messages = rows
            .into_iter()
            .map(|row| GypChatMessage {
                id: row.get("id"),
                project_id: row.get("project_id"),
                run_name: row.get("run_name"),
                role: row.get("role"),
                timestamp: row.get("timestamp"),
                chunks_json: row.get("chunks_json"),
            })
            .collect();

        Ok(messages)
    }

    /// Get messages for a project (all runs or no run)
    pub async fn get_project_messages(
        &self,
        project_id: i64,
    ) -> GypChatResult<Vec<GypChatMessage>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            "SELECT id, project_id, run_name, role, timestamp, chunks_json
             FROM gyp_chat_messages
             WHERE project_id = ?
             ORDER BY timestamp ASC",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        let messages = rows
            .into_iter()
            .map(|row| GypChatMessage {
                id: row.get("id"),
                project_id: row.get("project_id"),
                run_name: row.get("run_name"),
                role: row.get("role"),
                timestamp: row.get("timestamp"),
                chunks_json: row.get("chunks_json"),
            })
            .collect();

        Ok(messages)
    }

    /// Clear all messages for a run (or no-run if run_name is None)
    pub async fn clear_messages(&self, run_name: Option<&str>) -> GypChatResult<()> {
        let pool = self.pool().await;

        if run_name.is_some() {
            sqlx::query("DELETE FROM gyp_chat_messages WHERE run_name = ?")
                .bind(run_name)
                .execute(pool)
                .await?;
        } else {
            sqlx::query(
                "DELETE FROM gyp_chat_messages WHERE run_name IS NULL AND project_id IS NULL",
            )
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    /// Clear all messages for a project
    pub async fn clear_project_messages(&self, project_id: i64) -> GypChatResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM gyp_chat_messages WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Delete all messages for a specific run (used when deleting a run)
    pub async fn delete_run_messages(&self, run_name: &str) -> GypChatResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM gyp_chat_messages WHERE run_name = ?")
            .bind(run_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    // ========== BOARD CHAT METHODS ==========
    // Board chat uses a special run_name sentinel: "__board__"
    // This allows board chat history to be stored separately from run-specific chats.

    /// Save a board chat message for a project
    pub async fn save_board_message(
        &self,
        project_id: i64,
        role: &str,
        chunks_json: &str,
    ) -> GypChatResult<i64> {
        self.save_message_with_project(Some(project_id), Some("__board__"), role, chunks_json)
            .await
    }

    /// Get board chat messages for a project (most recent first)
    pub async fn get_board_messages(
        &self,
        project_id: i64,
        limit: usize,
    ) -> GypChatResult<Vec<GypChatMessage>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            "SELECT id, project_id, run_name, role, timestamp, chunks_json
             FROM gyp_chat_messages
             WHERE project_id = ? AND run_name = '__board__'
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(project_id)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;

        let messages: Vec<GypChatMessage> = rows
            .into_iter()
            .map(|row| GypChatMessage {
                id: row.get("id"),
                project_id: row.get("project_id"),
                run_name: row.get("run_name"),
                role: row.get("role"),
                timestamp: row.get("timestamp"),
                chunks_json: row.get("chunks_json"),
            })
            .collect();

        // Reverse to get chronological order (oldest first)
        Ok(messages.into_iter().rev().collect())
    }

    /// Clear all board chat messages for a project
    pub async fn clear_board_messages(&self, project_id: i64) -> GypChatResult<()> {
        let pool = self.pool().await;

        sqlx::query(
            "DELETE FROM gyp_chat_messages WHERE project_id = ? AND run_name = '__board__'",
        )
        .bind(project_id)
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
