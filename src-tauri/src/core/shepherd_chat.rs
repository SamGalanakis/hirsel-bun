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

CREATE TABLE IF NOT EXISTS shepherd_chat_queue (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,
    runtime_name TEXT,
    chunks_json TEXT NOT NULL,
    focus_json TEXT,
    status TEXT NOT NULL,
    error TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_shepherd_chat_queue_scope
ON shepherd_chat_queue(project_id, runtime_name, status, created_at);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdQueuedTurn {
    pub id: i64,
    pub project_id: Option<i64>,
    pub runtime_name: Option<String>,
    pub chunks_json: String,
    pub focus_json: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
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
    pub const PROJECT_CHAT_RUN_NAME: &str = "__project__";

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

    pub async fn enqueue_turn(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
        chunks_json: &str,
        focus_json: Option<&str>,
    ) -> ShepherdChatResult<ShepherdQueuedTurn> {
        let pool = self.pool().await;
        let created_at = utc_now();
        let scope_runtime = match (project_id, runtime_name) {
            (Some(_), None) => Some(Self::PROJECT_CHAT_RUN_NAME),
            (_, value) => value,
        };

        let result = sqlx::query(
            "INSERT INTO shepherd_chat_queue (
                project_id, runtime_name, chunks_json, focus_json, status, error, created_at
             ) VALUES (?, ?, ?, ?, 'pending', NULL, ?)",
        )
        .bind(project_id)
        .bind(scope_runtime)
        .bind(chunks_json)
        .bind(focus_json)
        .bind(&created_at)
        .execute(pool)
        .await?;

        self.get_queue_item(result.last_insert_rowid()).await
    }

    pub async fn get_queue_item(&self, id: i64) -> ShepherdChatResult<ShepherdQueuedTurn> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT id, project_id, runtime_name, chunks_json, focus_json, status, error, created_at, started_at, finished_at
             FROM shepherd_chat_queue
             WHERE id = ?",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(ShepherdQueuedTurn {
            id: row.get("id"),
            project_id: row.get("project_id"),
            runtime_name: row.get("runtime_name"),
            chunks_json: row.get("chunks_json"),
            focus_json: row.get("focus_json"),
            status: row.get("status"),
            error: row.get("error"),
            created_at: row.get("created_at"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
        })
    }

    pub async fn list_queue(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<Vec<ShepherdQueuedTurn>> {
        let pool = self.pool().await;
        let scope_runtime = match (project_id, runtime_name) {
            (Some(_), None) => Some(Self::PROJECT_CHAT_RUN_NAME),
            (_, value) => value,
        };

        let rows = if project_id.is_some() || scope_runtime.is_some() {
            sqlx::query(
                "SELECT id, project_id, runtime_name, chunks_json, focus_json, status, error, created_at, started_at, finished_at
                 FROM shepherd_chat_queue
                 WHERE project_id IS ? AND runtime_name IS ?
                   AND status IN ('pending', 'working', 'failed')
                 ORDER BY created_at ASC, id ASC",
            )
            .bind(project_id)
            .bind(scope_runtime)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, project_id, runtime_name, chunks_json, focus_json, status, error, created_at, started_at, finished_at
                 FROM shepherd_chat_queue
                 WHERE project_id IS NULL AND runtime_name IS NULL
                   AND status IN ('pending', 'working', 'failed')
                 ORDER BY created_at ASC, id ASC",
            )
            .fetch_all(pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|row| ShepherdQueuedTurn {
                id: row.get("id"),
                project_id: row.get("project_id"),
                runtime_name: row.get("runtime_name"),
                chunks_json: row.get("chunks_json"),
                focus_json: row.get("focus_json"),
                status: row.get("status"),
                error: row.get("error"),
                created_at: row.get("created_at"),
                started_at: row.get("started_at"),
                finished_at: row.get("finished_at"),
            })
            .collect())
    }

    pub async fn claim_next_turn(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<Option<ShepherdQueuedTurn>> {
        let pool = self.pool().await;
        let scope_runtime = match (project_id, runtime_name) {
            (Some(_), None) => Some(Self::PROJECT_CHAT_RUN_NAME),
            (_, value) => value,
        };

        let row = if project_id.is_some() || scope_runtime.is_some() {
            sqlx::query(
                "SELECT id
                 FROM shepherd_chat_queue
                 WHERE project_id IS ? AND runtime_name IS ? AND status = 'pending'
                 ORDER BY created_at ASC, id ASC
                 LIMIT 1",
            )
            .bind(project_id)
            .bind(scope_runtime)
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id
                 FROM shepherd_chat_queue
                 WHERE project_id IS NULL AND runtime_name IS NULL AND status = 'pending'
                 ORDER BY created_at ASC, id ASC
                 LIMIT 1",
            )
            .fetch_optional(pool)
            .await?
        };

        let Some(row) = row else {
            return Ok(None);
        };

        let id: i64 = row.get("id");
        let started_at = utc_now();
        sqlx::query(
            "UPDATE shepherd_chat_queue
             SET status = 'working', error = NULL, started_at = ?, finished_at = NULL
             WHERE id = ?",
        )
        .bind(&started_at)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(Some(self.get_queue_item(id).await?))
    }

    pub async fn complete_turn(&self, id: i64) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let finished_at = utc_now();
        sqlx::query(
            "UPDATE shepherd_chat_queue
             SET status = 'done', error = NULL, finished_at = ?
             WHERE id = ?",
        )
        .bind(&finished_at)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn fail_turn(&self, id: i64, error: &str) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let finished_at = utc_now();
        sqlx::query(
            "UPDATE shepherd_chat_queue
             SET status = 'failed', error = ?, finished_at = ?
             WHERE id = ?",
        )
        .bind(error)
        .bind(&finished_at)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn clear_queue(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let scope_runtime = match (project_id, runtime_name) {
            (Some(_), None) => Some(Self::PROJECT_CHAT_RUN_NAME),
            (_, value) => value,
        };

        if project_id.is_some() || scope_runtime.is_some() {
            sqlx::query(
                "DELETE FROM shepherd_chat_queue
                 WHERE project_id IS ? AND runtime_name IS ?",
            )
            .bind(project_id)
            .bind(scope_runtime)
            .execute(pool)
            .await?;
        } else {
            sqlx::query(
                "DELETE FROM shepherd_chat_queue
                 WHERE project_id IS NULL AND runtime_name IS NULL",
            )
            .execute(pool)
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
