//! Shepherd Chat History Storage
//!
//! Stores Shepherd (AI assistant) chat history in the global hirsel database.
//! Chat history is associated with runtime names to maintain separate scoped conversations.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::db::{global_pool, utc_now};

/// Schema for Shepherd chat tables
const SCHEMA: &str = r#"
-- Shepherd chat messages for persistent runtime-scoped AI assistant history
-- Supports three scopes:
-- 1. project_id=NULL, runtime_name=NULL → general chat
-- 2. project_id=X, runtime_name=NULL → project-level chat
-- 3. project_id=X, runtime_name=Y → runtime-specific chat
CREATE TABLE IF NOT EXISTS shepherd_chat_messages (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,       -- NULL for general conversations
    runtime_name TEXT,            -- NULL for project-level or general conversations
    role TEXT NOT NULL,       -- 'user', 'assistant', 'system'
    timestamp TEXT NOT NULL,
    chunks_json TEXT NOT NULL -- JSON-encoded message chunks
);

CREATE INDEX IF NOT EXISTS idx_shepherd_chat_run ON shepherd_chat_messages(runtime_name);
CREATE INDEX IF NOT EXISTS idx_shepherd_chat_project ON shepherd_chat_messages(project_id);
CREATE INDEX IF NOT EXISTS idx_shepherd_chat_timestamp ON shepherd_chat_messages(timestamp);
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

/// Shepherd chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdChatMessage {
    pub id: i64,
    pub project_id: Option<i64>,
    pub runtime_name: Option<String>,
    pub role: String,
    pub timestamp: String,
    pub chunks_json: String,
}

/// Error type for Shepherd chat operations
#[derive(Debug, thiserror::Error)]
pub enum ShepherdChatError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ShepherdChatResult<T> = Result<T, ShepherdChatError>;

/// Shepherd chat storage backed by SQLite
pub struct ShepherdChatStore;

impl ShepherdChatStore {
    /// Open the global Shepherd chat store
    pub async fn open() -> ShepherdChatResult<Self> {
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
        runtime_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(None, runtime_name, role, chunks_json)
            .await
    }

    /// Save a chat message with project context
    pub async fn save_message_with_project(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        let pool = self.pool().await;
        let timestamp = utc_now();

        let result = sqlx::query(
            "INSERT INTO shepherd_chat_messages (project_id, runtime_name, role, timestamp, chunks_json) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(runtime_name)
        .bind(role)
        .bind(&timestamp)
        .bind(chunks_json)
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get all messages for a runtime (or general chat if runtime_name is None).
    pub async fn get_messages(
        &self,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let pool = self.pool().await;

        let rows = if runtime_name.is_some() {
            sqlx::query(
                "SELECT id, project_id, runtime_name, role, timestamp, chunks_json
                 FROM shepherd_chat_messages
                 WHERE runtime_name = ?
                 ORDER BY timestamp ASC",
            )
            .bind(runtime_name)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, project_id, runtime_name, role, timestamp, chunks_json
                 FROM shepherd_chat_messages
                 WHERE runtime_name IS NULL AND project_id IS NULL
                 ORDER BY timestamp ASC",
            )
            .fetch_all(pool)
            .await?
        };

        let messages = rows
            .into_iter()
            .map(|row| ShepherdChatMessage {
                id: row.get("id"),
                project_id: row.get("project_id"),
                runtime_name: row.get("runtime_name"),
                role: row.get("role"),
                timestamp: row.get("timestamp"),
                chunks_json: row.get("chunks_json"),
            })
            .collect();

        Ok(messages)
    }

    /// Clear all messages for a runtime (or general chat if runtime_name is None).
    pub async fn clear_messages(&self, runtime_name: Option<&str>) -> ShepherdChatResult<()> {
        let pool = self.pool().await;

        if runtime_name.is_some() {
            sqlx::query("DELETE FROM shepherd_chat_messages WHERE runtime_name = ?")
                .bind(runtime_name)
                .execute(pool)
                .await?;
        } else {
            sqlx::query(
                "DELETE FROM shepherd_chat_messages WHERE runtime_name IS NULL AND project_id IS NULL",
            )
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    const PROJECT_CHAT_RUN_NAME: &str = "__project__";

    /// Clear project-scoped Shepherd history without touching runtime-scoped history.
    pub async fn clear_project_history(&self, project_id: i64) -> ShepherdChatResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM shepherd_chat_messages WHERE project_id = ? AND runtime_name = ?")
            .bind(project_id)
            .bind(Self::PROJECT_CHAT_RUN_NAME)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Delete every Shepherd message associated with a project, including
    /// project-scoped and runtime-scoped rows.
    pub async fn delete_project_messages(&self, project_id: i64) -> ShepherdChatResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM shepherd_chat_messages WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Delete all messages for a specific runtime.
    pub async fn delete_run_messages(&self, runtime_name: &str) -> ShepherdChatResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM shepherd_chat_messages WHERE runtime_name = ?")
            .bind(runtime_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    // ========== PROJECT CHAT METHODS ==========
    // Project-scoped Shepherd chat uses a dedicated runtime_name sentinel so it stays
    // separate from runtime-specific history.

    /// Save a project-scoped Shepherd message for a project.
    pub async fn save_project_message(
        &self,
        project_id: i64,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(
            Some(project_id),
            Some(Self::PROJECT_CHAT_RUN_NAME),
            role,
            chunks_json,
        )
        .await
    }

    /// Get project-scoped Shepherd messages for a project (most recent first).
    pub async fn get_project_messages(
        &self,
        project_id: i64,
        limit: usize,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            "SELECT id, project_id, runtime_name, role, timestamp, chunks_json
             FROM shepherd_chat_messages
             WHERE project_id = ? AND runtime_name = ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(project_id)
        .bind(Self::PROJECT_CHAT_RUN_NAME)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;

        let messages: Vec<ShepherdChatMessage> = rows
            .into_iter()
            .map(|row| ShepherdChatMessage {
                id: row.get("id"),
                project_id: row.get("project_id"),
                runtime_name: row.get("runtime_name"),
                role: row.get("role"),
                timestamp: row.get("timestamp"),
                chunks_json: row.get("chunks_json"),
            })
            .collect();

        // Reverse to get chronological order (oldest first)
        Ok(messages.into_iter().rev().collect())
    }

    /// Clear all project-scoped Shepherd messages for a project.
    pub async fn clear_project_messages(&self, project_id: i64) -> ShepherdChatResult<()> {
        self.clear_project_history(project_id).await
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
