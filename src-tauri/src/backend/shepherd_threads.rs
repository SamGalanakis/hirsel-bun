use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use crate::backend::db::{global_pool, utc_now};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS shepherd_threads (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    objective TEXT NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL,
    workspace_path TEXT,
    checkout_name TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_activity_at TEXT NOT NULL,
    archived_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_shepherd_threads_project
ON shepherd_threads(project_id, archived_at, last_activity_at);
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
pub struct ShepherdThread {
    pub id: String,
    pub project_id: i64,
    pub title: String,
    pub objective: String,
    pub summary: String,
    pub status: String,
    pub workspace_path: Option<String>,
    pub checkout_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShepherdThreadError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub type ShepherdThreadResult<T> = Result<T, ShepherdThreadError>;

pub struct ShepherdThreadStore;

impl ShepherdThreadStore {
    pub fn scope_key(thread_id: &str) -> String {
        format!("__thread__:{thread_id}")
    }

    pub async fn open() -> ShepherdThreadResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    fn row_to_thread(row: sqlx::sqlite::SqliteRow) -> ShepherdThread {
        ShepherdThread {
            id: row.get("id"),
            project_id: row.get("project_id"),
            title: row.get("title"),
            objective: row.get("objective"),
            summary: row.get("summary"),
            status: row.get("status"),
            workspace_path: row.get("workspace_path"),
            checkout_name: row.get("checkout_name"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            last_activity_at: row.get("last_activity_at"),
            archived_at: row.get("archived_at"),
        }
    }

    pub async fn create_thread(
        &self,
        project_id: i64,
        title: &str,
        objective: &str,
        summary: &str,
        workspace_path: Option<&str>,
        checkout_name: Option<&str>,
    ) -> ShepherdThreadResult<ShepherdThread> {
        let pool = self.pool().await;
        let id = uuid::Uuid::new_v4().to_string();
        let now = utc_now();

        sqlx::query(
            "INSERT INTO shepherd_threads (
                id, project_id, title, objective, summary, status,
                workspace_path, checkout_name, created_at, updated_at, last_activity_at, archived_at
             ) VALUES (?, ?, ?, ?, ?, 'running', ?, ?, ?, ?, ?, NULL)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(title)
        .bind(objective)
        .bind(summary)
        .bind(workspace_path)
        .bind(checkout_name)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        self.get_thread(&id).await
    }

    pub async fn get_thread(&self, thread_id: &str) -> ShepherdThreadResult<ShepherdThread> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT id, project_id, title, objective, summary, status,
                    workspace_path, checkout_name, created_at, updated_at, last_activity_at, archived_at
             FROM shepherd_threads
             WHERE id = ?",
        )
        .bind(thread_id)
        .fetch_one(pool)
        .await?;
        Ok(Self::row_to_thread(row))
    }

    pub async fn list_project_threads(
        &self,
        project_id: i64,
    ) -> ShepherdThreadResult<Vec<ShepherdThread>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, project_id, title, objective, summary, status,
                    workspace_path, checkout_name, created_at, updated_at, last_activity_at, archived_at
             FROM shepherd_threads
             WHERE project_id = ? AND archived_at IS NULL
             ORDER BY
                CASE status
                    WHEN 'running' THEN 0
                    WHEN 'waiting' THEN 1
                    WHEN 'blocked' THEN 2
                    WHEN 'failed' THEN 3
                    WHEN 'done' THEN 4
                    ELSE 5
                END,
                last_activity_at DESC,
                created_at DESC",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(Self::row_to_thread).collect())
    }

    pub async fn find_project_thread_by_title(
        &self,
        project_id: i64,
        title: &str,
    ) -> ShepherdThreadResult<Option<ShepherdThread>> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT id, project_id, title, objective, summary, status,
                    workspace_path, checkout_name, created_at, updated_at, last_activity_at, archived_at
             FROM shepherd_threads
             WHERE project_id = ? AND archived_at IS NULL AND lower(title) = lower(?)
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(project_id)
        .bind(title)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Self::row_to_thread))
    }

    pub async fn update_thread(
        &self,
        thread_id: &str,
        title: Option<&str>,
        objective: Option<&str>,
        summary: Option<&str>,
        status: Option<&str>,
        workspace_path: Option<Option<&str>>,
        checkout_name: Option<Option<&str>>,
    ) -> ShepherdThreadResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "UPDATE shepherd_threads
             SET title = COALESCE(?, title),
                 objective = COALESCE(?, objective),
                 summary = COALESCE(?, summary),
                 status = COALESCE(?, status),
                 workspace_path = COALESCE(?, workspace_path),
                 checkout_name = COALESCE(?, checkout_name),
                 updated_at = ?,
                 last_activity_at = ?
             WHERE id = ?",
        )
        .bind(title)
        .bind(objective)
        .bind(summary)
        .bind(status)
        .bind(workspace_path.flatten())
        .bind(checkout_name.flatten())
        .bind(&now)
        .bind(&now)
        .bind(thread_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn touch_thread(&self, thread_id: &str) -> ShepherdThreadResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "UPDATE shepherd_threads
             SET updated_at = ?, last_activity_at = ?
             WHERE id = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(thread_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn archive_thread(&self, thread_id: &str) -> ShepherdThreadResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "UPDATE shepherd_threads
             SET archived_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(thread_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete_thread(&self, thread_id: &str) -> ShepherdThreadResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_threads WHERE id = ?")
            .bind(thread_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_project_threads(&self, project_id: i64) -> ShepherdThreadResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_threads WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
