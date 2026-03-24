//! Shepherd chat and effort storage.

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

CREATE TABLE IF NOT EXISTS shepherd_efforts (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    work_item_id TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL,
    focused INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_activity_at TEXT NOT NULL,
    archived_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_shepherd_efforts_project_route
ON shepherd_efforts(project_id, route_id, archived_at, focused, last_activity_at);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdEffort {
    pub id: String,
    pub project_id: i64,
    pub route_id: i64,
    pub work_item_id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub focused: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
    pub archived_at: Option<String>,
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
    pub const PROJECT_CHAT_RUN_NAME: &str = "__project__";

    pub fn effort_runtime_name(effort_id: &str) -> String {
        format!("__effort__:{effort_id}")
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

    fn row_to_effort(row: sqlx::sqlite::SqliteRow) -> ShepherdEffort {
        ShepherdEffort {
            id: row.get("id"),
            project_id: row.get("project_id"),
            route_id: row.get("route_id"),
            work_item_id: row.get("work_item_id"),
            title: row.get("title"),
            summary: row.get("summary"),
            status: row.get("status"),
            focused: row.get::<i64, _>("focused") != 0,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            last_activity_at: row.get("last_activity_at"),
            archived_at: row.get("archived_at"),
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

    pub async fn clear_messages(&self, runtime_name: Option<&str>) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "DELETE FROM shepherd_chat_messages WHERE project_id IS NULL AND runtime_name IS ?",
        )
        .bind(runtime_name)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn clear_scope_messages(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "DELETE FROM shepherd_chat_messages WHERE project_id IS ? AND runtime_name IS ?",
        )
        .bind(project_id)
        .bind(runtime_name)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn clear_project_history(&self, project_id: i64) -> ShepherdChatResult<()> {
        self.clear_scope_messages(Some(project_id), Some(Self::PROJECT_CHAT_RUN_NAME))
            .await
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
        sqlx::query("DELETE FROM shepherd_efforts WHERE project_id = ?")
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
        Ok(())
    }

    pub async fn save_project_message(
        &self,
        project_id: i64,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_scope_message(
            Some(project_id),
            Some(Self::PROJECT_CHAT_RUN_NAME),
            role,
            chunks_json,
        )
        .await
    }

    pub async fn get_project_messages(
        &self,
        project_id: i64,
        limit: usize,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        self.get_scope_messages(Some(project_id), Some(Self::PROJECT_CHAT_RUN_NAME), limit)
            .await
    }

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

    pub async fn clear_queue(
        &self,
        project_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_chat_queue WHERE project_id IS ? AND runtime_name IS ?")
            .bind(project_id)
            .bind(runtime_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn create_effort(
        &self,
        project_id: i64,
        route_id: i64,
        work_item_id: &str,
        title: &str,
        summary: &str,
        focused: bool,
    ) -> ShepherdChatResult<ShepherdEffort> {
        let pool = self.pool().await;
        let id = uuid::Uuid::new_v4().to_string();
        let now = utc_now();
        let mut tx = pool.begin().await?;

        if focused {
            sqlx::query("UPDATE shepherd_efforts SET focused = 0 WHERE project_id = ?")
                .bind(project_id)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query(
            "INSERT INTO shepherd_efforts (
                id, project_id, route_id, work_item_id, title, summary, status, focused,
                created_at, updated_at, last_activity_at, archived_at
             ) VALUES (?, ?, ?, ?, ?, ?, 'active', ?, ?, ?, ?, NULL)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(route_id)
        .bind(work_item_id)
        .bind(title)
        .bind(summary)
        .bind(if focused { 1 } else { 0 })
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.get_effort(&id).await
    }

    pub async fn list_project_efforts(
        &self,
        project_id: i64,
        route_id: i64,
    ) -> ShepherdChatResult<Vec<ShepherdEffort>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, project_id, route_id, work_item_id, title, summary, status, focused,
                    created_at, updated_at, last_activity_at, archived_at
             FROM shepherd_efforts
             WHERE project_id = ? AND route_id = ? AND archived_at IS NULL
             ORDER BY focused DESC, last_activity_at DESC, created_at DESC",
        )
        .bind(project_id)
        .bind(route_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(Self::row_to_effort).collect())
    }

    pub async fn get_effort(&self, effort_id: &str) -> ShepherdChatResult<ShepherdEffort> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT id, project_id, route_id, work_item_id, title, summary, status, focused,
                    created_at, updated_at, last_activity_at, archived_at
             FROM shepherd_efforts
             WHERE id = ?",
        )
        .bind(effort_id)
        .fetch_one(pool)
        .await?;
        Ok(Self::row_to_effort(row))
    }

    pub async fn focused_effort(
        &self,
        project_id: i64,
        route_id: i64,
    ) -> ShepherdChatResult<Option<ShepherdEffort>> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT id, project_id, route_id, work_item_id, title, summary, status, focused,
                    created_at, updated_at, last_activity_at, archived_at
             FROM shepherd_efforts
             WHERE project_id = ? AND route_id = ? AND archived_at IS NULL AND focused = 1
             ORDER BY last_activity_at DESC
             LIMIT 1",
        )
        .bind(project_id)
        .bind(route_id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Self::row_to_effort))
    }

    pub async fn set_focused_effort(
        &self,
        project_id: i64,
        effort_id: &str,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE shepherd_efforts SET focused = 0 WHERE project_id = ?")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE shepherd_efforts
             SET focused = 1, updated_at = ?, last_activity_at = ?
             WHERE id = ? AND project_id = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(effort_id)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_effort(
        &self,
        effort_id: &str,
        summary: Option<&str>,
        status: Option<&str>,
    ) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "UPDATE shepherd_efforts
             SET summary = COALESCE(?, summary),
                 status = COALESCE(?, status),
                 updated_at = ?,
                 last_activity_at = ?
             WHERE id = ?",
        )
        .bind(summary)
        .bind(status)
        .bind(&now)
        .bind(&now)
        .bind(effort_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn archive_effort(&self, effort_id: &str) -> ShepherdChatResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "UPDATE shepherd_efforts
             SET archived_at = ?, focused = 0, updated_at = ?
             WHERE id = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(effort_id)
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {}
