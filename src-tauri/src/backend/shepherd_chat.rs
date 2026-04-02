//! Shepherd chat, live-turn, and scope-state storage.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::db::{global_pool, utc_now};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS shepherd_chat_messages (
    id INTEGER PRIMARY KEY,
    project_id INTEGER,
    scope_key TEXT,
    role TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    chunks_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_shepherd_chat_scope
ON shepherd_chat_messages(project_id, scope_key, timestamp);

CREATE TABLE IF NOT EXISTS shepherd_live_turns (
    project_id INTEGER,
    scope_key TEXT NOT NULL,
    role TEXT NOT NULL,
    chunks_json TEXT NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (project_id, scope_key)
);

CREATE INDEX IF NOT EXISTS idx_shepherd_live_turns_scope
ON shepherd_live_turns(project_id, scope_key, updated_at);

CREATE TABLE IF NOT EXISTS shepherd_scope_states (
    project_id INTEGER NOT NULL,
    scope_key TEXT NOT NULL,
    state_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (project_id, scope_key)
);

CREATE INDEX IF NOT EXISTS idx_shepherd_scope_states_project_scope
ON shepherd_scope_states(project_id, scope_key, updated_at);
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
    pub scope_key: Option<String>,
    pub role: String,
    pub timestamp: String,
    pub chunks_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdLiveTurn {
    pub project_id: Option<i64>,
    pub scope_key: String,
    pub role: String,
    pub chunks_json: String,
    pub status: String,
    pub error: Option<String>,
    pub started_at: String,
    pub updated_at: String,
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
    pub fn shepherd_scope_key(project_id: i64) -> String {
        format!("__shepherd__:{project_id}")
    }

    pub fn thread_scope_key(thread_id: &str) -> String {
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
            scope_key: row.get("scope_key"),
            role: row.get("role"),
            timestamp: row.get("timestamp"),
            chunks_json: row.get("chunks_json"),
        }
    }

    fn row_to_live_turn(row: sqlx::sqlite::SqliteRow) -> ShepherdLiveTurn {
        ShepherdLiveTurn {
            project_id: row.get("project_id"),
            scope_key: row.get("scope_key"),
            role: row.get("role"),
            chunks_json: row.get("chunks_json"),
            status: row.get("status"),
            error: row.get("error"),
            started_at: row.get("started_at"),
            updated_at: row.get("updated_at"),
        }
    }

    pub async fn save_message(
        &self,
        scope_key: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(None, scope_key, role, chunks_json)
            .await
    }

    pub async fn save_message_with_project(
        &self,
        project_id: Option<i64>,
        scope_key: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        let pool = self.pool().await;
        let timestamp = utc_now();
        let result = sqlx::query(
            "INSERT INTO shepherd_chat_messages (project_id, scope_key, role, timestamp, chunks_json)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(scope_key)
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
        scope_key: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(project_id, scope_key, role, chunks_json)
            .await
    }

    pub async fn get_scope_messages(
        &self,
        project_id: Option<i64>,
        scope_key: Option<&str>,
        limit: usize,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, project_id, scope_key, role, timestamp, chunks_json
             FROM shepherd_chat_messages
             WHERE project_id IS ? AND scope_key IS ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(project_id)
        .bind(scope_key)
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
        scope_key: Option<&str>,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let limit = i64::MAX as usize;
        self.get_scope_messages(None, scope_key, limit).await
    }

    pub async fn delete_project_messages(&self, project_id: i64) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_chat_messages WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_live_turns WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_scope_states WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_scope_messages(&self, scope_key: &str) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_chat_messages WHERE scope_key = ?")
            .bind(scope_key)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_live_turns WHERE scope_key = ?")
            .bind(scope_key)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM shepherd_scope_states WHERE scope_key = ?")
            .bind(scope_key)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn get_scope_state(
        &self,
        project_id: i64,
        scope_key: &str,
    ) -> ShepherdChatResult<Option<String>> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT state_json
             FROM shepherd_scope_states
             WHERE project_id = ? AND scope_key = ?",
        )
        .bind(project_id)
        .bind(scope_key)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| row.get("state_json")))
    }

    pub async fn save_scope_state(
        &self,
        project_id: i64,
        scope_key: &str,
        state_json: &str,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "INSERT INTO shepherd_scope_states (project_id, scope_key, state_json, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(project_id, scope_key)
             DO UPDATE SET
                state_json = excluded.state_json,
                updated_at = excluded.updated_at",
        )
        .bind(project_id)
        .bind(scope_key)
        .bind(state_json)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn clear_scope_state(
        &self,
        project_id: i64,
        scope_key: &str,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "DELETE FROM shepherd_scope_states
             WHERE project_id = ? AND scope_key = ?",
        )
        .bind(project_id)
        .bind(scope_key)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_live_turn(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
    ) -> ShepherdChatResult<Option<ShepherdLiveTurn>> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT project_id, scope_key, role, chunks_json, status, error, started_at, updated_at
             FROM shepherd_live_turns
             WHERE project_id IS ? AND scope_key = ?",
        )
        .bind(project_id)
        .bind(scope_key)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Self::row_to_live_turn))
    }

    pub async fn save_live_turn(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
        role: &str,
        chunks_json: &str,
        status: &str,
        error: Option<&str>,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "INSERT INTO shepherd_live_turns (
                project_id, scope_key, role, chunks_json, status, error, started_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(project_id, scope_key)
             DO UPDATE SET
                role = excluded.role,
                chunks_json = excluded.chunks_json,
                status = excluded.status,
                error = excluded.error,
                updated_at = excluded.updated_at",
        )
        .bind(project_id)
        .bind(scope_key)
        .bind(role)
        .bind(chunks_json)
        .bind(status)
        .bind(error)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn clear_live_turn(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "DELETE FROM shepherd_live_turns
             WHERE project_id IS ? AND scope_key = ?",
        )
        .bind(project_id)
        .bind(scope_key)
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {}
