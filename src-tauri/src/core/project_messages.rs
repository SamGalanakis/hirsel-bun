//! Project Messages Storage (Sheepfold)
//!
//! Stores project-scoped messaging in the global hirsel database.
//! Supports:
//! - Meadow (group chat with all workers + human)
//! - Worker DMs (direct messages via worker name threads)

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::db::{global_pool, utc_now};

/// Schema for project messages tables
const SCHEMA: &str = r#"
-- Project-scoped messages for Sheepfold
-- thread = 'meadow' for group chat, or worker_name for DMs
-- route_id scopes messages to a specific route (default 0 for backwards compatibility)
CREATE TABLE IF NOT EXISTS project_messages (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL DEFAULT 0,
    thread TEXT NOT NULL,
    sender TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    waiting INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_project_messages_project ON project_messages(project_id);
CREATE INDEX IF NOT EXISTS idx_project_messages_route ON project_messages(project_id, route_id);
CREATE INDEX IF NOT EXISTS idx_project_messages_thread ON project_messages(project_id, route_id, thread);
CREATE INDEX IF NOT EXISTS idx_project_messages_timestamp ON project_messages(timestamp);

-- Read tracking for unread counts
CREATE TABLE IF NOT EXISTS project_message_reads (
    reader TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL DEFAULT 0,
    thread TEXT NOT NULL,
    last_read_id INTEGER NOT NULL,
    PRIMARY KEY (reader, project_id, route_id, thread)
);
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

/// Project message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMessage {
    pub id: i64,
    pub project_id: i64,
    pub route_id: i64,
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
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ProjectMessagesResult<T> = Result<T, ProjectMessagesError>;

/// Project messages storage backed by SQLite
pub struct ProjectMessagesStore;

impl ProjectMessagesStore {
    /// Open the global project messages store
    pub async fn open() -> ProjectMessagesResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    /// Get the pool
    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    /// Add a message to a thread
    pub async fn add_message(
        &self,
        project_id: i64,
        route_id: i64,
        thread: &str,
        sender: &str,
        content: &str,
        waiting: bool,
    ) -> ProjectMessagesResult<ProjectMessage> {
        let pool = self.pool().await;
        let timestamp = utc_now();
        let waiting_int = if waiting { 1i64 } else { 0i64 };

        let result = sqlx::query(
            "INSERT INTO project_messages (project_id, route_id, thread, sender, content, timestamp, waiting)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(route_id)
        .bind(thread)
        .bind(sender)
        .bind(content)
        .bind(&timestamp)
        .bind(waiting_int)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();

        Ok(ProjectMessage {
            id,
            project_id,
            route_id,
            thread: thread.to_string(),
            sender: sender.to_string(),
            content: content.to_string(),
            waiting,
            timestamp,
        })
    }

    /// Get messages for a thread
    pub async fn get_messages(
        &self,
        project_id: i64,
        route_id: i64,
        thread: &str,
        limit: Option<i64>,
    ) -> ProjectMessagesResult<Vec<ProjectMessage>> {
        let pool = self.pool().await;
        let limit = limit.unwrap_or(100);

        let rows = sqlx::query(
            "SELECT id, project_id, route_id, thread, sender, content, timestamp, waiting
             FROM project_messages
             WHERE project_id = ? AND route_id = ? AND thread = ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(project_id)
        .bind(route_id)
        .bind(thread)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let messages: Vec<ProjectMessage> = rows
            .into_iter()
            .map(|row| ProjectMessage {
                id: row.get("id"),
                project_id: row.get("project_id"),
                route_id: row.get("route_id"),
                thread: row.get("thread"),
                sender: row.get("sender"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                waiting: row.get::<i64, _>("waiting") != 0,
            })
            .collect();

        // Reverse to get chronological order (oldest first)
        Ok(messages.into_iter().rev().collect())
    }

    /// Get all threads for a project route with unread counts
    pub async fn get_threads(
        &self,
        project_id: i64,
        route_id: i64,
        reader: &str,
    ) -> ProjectMessagesResult<Vec<ProjectThreadSummary>> {
        let pool = self.pool().await;

        let rows = sqlx::query(
            r#"
            SELECT
                m.thread,
                COUNT(*) as message_count,
                (SELECT content FROM project_messages
                 WHERE project_id = m.project_id AND route_id = m.route_id AND thread = m.thread
                 ORDER BY timestamp DESC LIMIT 1) as last_message,
                (SELECT timestamp FROM project_messages
                 WHERE project_id = m.project_id AND route_id = m.route_id AND thread = m.thread
                 ORDER BY timestamp DESC LIMIT 1) as last_timestamp,
                COALESCE(
                    (SELECT COUNT(*) FROM project_messages pm
                     WHERE pm.project_id = m.project_id
                       AND pm.route_id = m.route_id
                       AND pm.thread = m.thread
                       AND pm.id > COALESCE(
                           (SELECT last_read_id FROM project_message_reads
                            WHERE reader = ? AND project_id = m.project_id AND route_id = m.route_id AND thread = m.thread),
                           0
                       )
                    ),
                    0
                ) as unread_count
            FROM project_messages m
            WHERE m.project_id = ? AND m.route_id = ?
            GROUP BY m.thread
            ORDER BY last_timestamp DESC
            "#,
        )
        .bind(reader)
        .bind(project_id)
        .bind(route_id)
        .fetch_all(pool)
        .await?;

        let threads = rows
            .into_iter()
            .map(|row| ProjectThreadSummary {
                thread: row.get("thread"),
                message_count: row.get("message_count"),
                unread_count: row.get("unread_count"),
                last_message: row.get("last_message"),
                last_timestamp: row.get("last_timestamp"),
            })
            .collect();

        Ok(threads)
    }

    /// Mark messages as read up to the latest message
    pub async fn mark_messages_read(
        &self,
        project_id: i64,
        route_id: i64,
        thread: &str,
        reader: &str,
    ) -> ProjectMessagesResult<()> {
        let pool = self.pool().await;

        // Get the latest message ID
        let last_id: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(id) FROM project_messages WHERE project_id = ? AND route_id = ? AND thread = ?",
        )
        .bind(project_id)
        .bind(route_id)
        .bind(thread)
        .fetch_one(pool)
        .await?;

        if let Some(id) = last_id {
            sqlx::query(
                "INSERT OR REPLACE INTO project_message_reads (reader, project_id, route_id, thread, last_read_id)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(reader)
            .bind(project_id)
            .bind(route_id)
            .bind(thread)
            .bind(id)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Get total unread count across all threads for a project route
    pub async fn get_unread_count(
        &self,
        project_id: i64,
        route_id: i64,
        reader: &str,
    ) -> ProjectMessagesResult<i64> {
        let pool = self.pool().await;

        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(SUM(
                (SELECT COUNT(*) FROM project_messages pm
                 WHERE pm.project_id = ?
                   AND pm.route_id = ?
                   AND pm.thread = threads.thread
                   AND pm.id > COALESCE(
                       (SELECT last_read_id FROM project_message_reads
                        WHERE reader = ? AND project_id = ? AND route_id = ? AND thread = threads.thread),
                       0
                   )
                )
            ), 0)
            FROM (SELECT DISTINCT thread FROM project_messages WHERE project_id = ? AND route_id = ?) threads
            "#,
        )
        .bind(project_id)
        .bind(route_id)
        .bind(reader)
        .bind(project_id)
        .bind(route_id)
        .bind(project_id)
        .bind(route_id)
        .fetch_one(pool)
        .await?;

        Ok(count)
    }

    /// Delete all messages for a project (called when project is deleted)
    pub async fn delete_project_messages(&self, project_id: i64) -> ProjectMessagesResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM project_messages WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM project_message_reads WHERE project_id = ?")
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
