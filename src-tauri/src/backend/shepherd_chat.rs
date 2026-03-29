//! Shepherd chat and scope-state storage.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::db::{global_pool, utc_now};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS shepherd_chat_messages (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,
    runtime_name TEXT,
    role TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    chunks_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_shepherd_chat_scope
ON shepherd_chat_messages(project_id, runtime_name, timestamp);

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

CREATE TABLE IF NOT EXISTS shepherd_scope_states (
    project_id INTEGER NOT NULL,
    runtime_name TEXT NOT NULL,
    state_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (project_id, runtime_name)
);

CREATE INDEX IF NOT EXISTS idx_shepherd_scope_states_project_runtime
ON shepherd_scope_states(project_id, runtime_name, updated_at);
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

#[derive(Debug, thiserror::Error)]
pub enum ShepherdChatError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ShepherdChatResult<T> = Result<T, ShepherdChatError>;

pub struct ShepherdChatStore;

impl ShepherdChatStore {
    pub fn project_runtime_name(route_id: i64) -> String {
        format!("__project__:{route_id}")
    }

    pub fn thread_runtime_name(thread_id: &str) -> String {
        format!("__thread__:{thread_id}")
    }

    pub async fn open() -> ShepherdChatResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    fn row_to_message(row: sqlx::sqlite::SqliteRow) -> ShepherdChatMessage {
        ShepherdChatMessage {
            id: row.get("id"),
            project_id: row.get("project_id"),
            runtime_name: row.get("runtime_name"),
            role: row.get("role"),
            timestamp: row.get("timestamp"),
            chunks_json: row.get("chunks_json"),
        }
    }

    fn row_to_queue_item(row: sqlx::sqlite::SqliteRow) -> ShepherdQueuedTurn {
        ShepherdQueuedTurn {
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
        }
    }

    pub async fn save_message(
        &self,
        runtime_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(None, runtime_name, role, chunks_json)
            .await
    }

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
            "INSERT INTO shepherd_chat_messages (project_id, runtime_name, role, timestamp, chunks_json)
             VALUES (?, ?, ?, ?, ?)",
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

    pub async fn save_scope_message(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(project_id, runtime_name, role, chunks_json)
            .await
    }

    pub async fn get_scope_messages(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
        limit: usize,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, project_id, runtime_name, role, timestamp, chunks_json
             FROM shepherd_chat_messages
             WHERE project_id IS ? AND runtime_name IS ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(project_id)
        .bind(runtime_name)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;

        let mut messages = rows
            .into_iter()
            .map(Self::row_to_message)
            .collect::<Vec<_>>();
        messages.reverse();
        Ok(messages)
    }

    pub async fn get_messages(
        &self,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let limit = i64::MAX as usize;
        self.get_scope_messages(None, runtime_name, limit).await
    }

    pub async fn delete_project_messages(&self, project_id: i64) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_chat_messages WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_chat_queue WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_scope_states WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_run_messages(&self, runtime_name: &str) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_chat_messages WHERE runtime_name = ?")
            .bind(runtime_name)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_chat_queue WHERE runtime_name = ?")
            .bind(runtime_name)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_scope_states WHERE runtime_name = ?")
            .bind(runtime_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn get_scope_state(
        &self,
        project_id: i64,
        runtime_name: &str,
    ) -> ShepherdChatResult<Option<String>> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT state_json
             FROM shepherd_scope_states
             WHERE project_id = ? AND runtime_name = ?",
        )
        .bind(project_id)
        .bind(runtime_name)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| row.get("state_json")))
    }

    pub async fn save_scope_state(
        &self,
        project_id: i64,
        runtime_name: &str,
        state_json: &str,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "INSERT INTO shepherd_scope_states (project_id, runtime_name, state_json, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(project_id, runtime_name)
             DO UPDATE SET
                state_json = excluded.state_json,
                updated_at = excluded.updated_at",
        )
        .bind(project_id)
        .bind(runtime_name)
        .bind(state_json)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn clear_scope_state(
        &self,
        project_id: i64,
        runtime_name: &str,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "DELETE FROM shepherd_scope_states
             WHERE project_id = ? AND runtime_name = ?",
        )
        .bind(project_id)
        .bind(runtime_name)
        .execute(pool)
        .await?;
        Ok(())
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
        let result = sqlx::query(
            "INSERT INTO shepherd_chat_queue (
                project_id, runtime_name, chunks_json, focus_json, status, error, created_at
             ) VALUES (?, ?, ?, ?, 'pending', NULL, ?)",
        )
        .bind(project_id)
        .bind(runtime_name)
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
        Ok(Self::row_to_queue_item(row))
    }

    pub async fn list_queue(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<Vec<ShepherdQueuedTurn>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, project_id, runtime_name, chunks_json, focus_json, status, error, created_at, started_at, finished_at
             FROM shepherd_chat_queue
             WHERE project_id IS ? AND runtime_name IS ?
               AND status IN ('pending', 'working', 'failed')
             ORDER BY created_at ASC, id ASC",
        )
        .bind(project_id)
        .bind(runtime_name)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(Self::row_to_queue_item).collect())
    }

    pub async fn claim_next_turn(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<Option<ShepherdQueuedTurn>> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT id
             FROM shepherd_chat_queue
             WHERE project_id IS ? AND runtime_name IS ? AND status = 'pending'
             ORDER BY created_at ASC, id ASC
             LIMIT 1",
        )
        .bind(project_id)
        .bind(runtime_name)
        .fetch_optional(pool)
        .await?;

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
}

#[cfg(test)]
mod tests {}
