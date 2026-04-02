use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use crate::backend::db::{global_pool, utc_now};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS shepherd_sessions (
    project_id INTEGER,
    scope_key TEXT NOT NULL PRIMARY KEY,
    scope_json TEXT NOT NULL,
    workspace_path TEXT,
    env_fingerprint TEXT,
    status TEXT NOT NULL,
    container_name TEXT,
    socket_path TEXT NOT NULL,
    bootstrap_flake INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_seen_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_shepherd_sessions_project
ON shepherd_sessions(project_id, updated_at);
"#;

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    SCHEMA_INIT
        .get_or_try_init(|| async {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
            if let Err(error) =
                sqlx::query("ALTER TABLE shepherd_sessions ADD COLUMN env_fingerprint TEXT")
                    .execute(pool)
                    .await
            {
                let duplicate_column = error
                    .to_string()
                    .to_ascii_lowercase()
                    .contains("duplicate column name");
                if !duplicate_column {
                    return Err(error);
                }
            }
            Ok::<(), sqlx::Error>(())
        })
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdScopeSession {
    pub project_id: Option<i64>,
    pub scope_key: String,
    pub scope_json: String,
    pub workspace_path: Option<String>,
    pub env_fingerprint: Option<String>,
    pub status: String,
    pub container_name: Option<String>,
    pub socket_path: String,
    pub bootstrap_flake: bool,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShepherdSessionError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub type ShepherdSessionResult<T> = Result<T, ShepherdSessionError>;

pub struct ShepherdSessionStore;

impl ShepherdSessionStore {
    pub async fn open() -> ShepherdSessionResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    fn row_to_session(row: sqlx::sqlite::SqliteRow) -> ShepherdScopeSession {
        ShepherdScopeSession {
            project_id: row.get("project_id"),
            scope_key: row.get("scope_key"),
            scope_json: row.get("scope_json"),
            workspace_path: row.get("workspace_path"),
            env_fingerprint: row.get("env_fingerprint"),
            status: row.get("status"),
            container_name: row.get("container_name"),
            socket_path: row.get("socket_path"),
            bootstrap_flake: row.get::<i64, _>("bootstrap_flake") != 0,
            last_error: row.get("last_error"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            last_seen_at: row.get("last_seen_at"),
        }
    }

    pub async fn get_session(
        &self,
        scope_key: &str,
    ) -> ShepherdSessionResult<Option<ShepherdScopeSession>> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT project_id, scope_key, scope_json, workspace_path, status, container_name,
                    env_fingerprint, socket_path, bootstrap_flake, last_error, created_at, updated_at, last_seen_at
             FROM shepherd_sessions
             WHERE scope_key = ?",
        )
        .bind(scope_key)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Self::row_to_session))
    }

    pub async fn upsert_session(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
        scope_json: &str,
        workspace_path: Option<&str>,
        env_fingerprint: Option<&str>,
        socket_path: &str,
        bootstrap_flake: bool,
        container_name: Option<&str>,
        status: &str,
        last_error: Option<&str>,
    ) -> ShepherdSessionResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "INSERT INTO shepherd_sessions (
                project_id, scope_key, scope_json, workspace_path, env_fingerprint, status,
                container_name, socket_path, bootstrap_flake, last_error, created_at, updated_at, last_seen_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
             ON CONFLICT(scope_key)
             DO UPDATE SET
                project_id = excluded.project_id,
                scope_json = excluded.scope_json,
                workspace_path = excluded.workspace_path,
                env_fingerprint = excluded.env_fingerprint,
                status = excluded.status,
                container_name = excluded.container_name,
                socket_path = excluded.socket_path,
                bootstrap_flake = excluded.bootstrap_flake,
                last_error = excluded.last_error,
                updated_at = excluded.updated_at",
        )
        .bind(project_id)
        .bind(scope_key)
        .bind(scope_json)
        .bind(workspace_path)
        .bind(env_fingerprint)
        .bind(status)
        .bind(container_name)
        .bind(socket_path)
        .bind(if bootstrap_flake { 1 } else { 0 })
        .bind(last_error)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn set_status(
        &self,
        scope_key: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> ShepherdSessionResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "UPDATE shepherd_sessions
             SET status = ?,
                 last_error = ?,
                 updated_at = ?
             WHERE scope_key = ?",
        )
        .bind(status)
        .bind(last_error)
        .bind(&now)
        .bind(scope_key)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn touch_seen(&self, scope_key: &str) -> ShepherdSessionResult<()> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            "UPDATE shepherd_sessions
             SET last_seen_at = ?, updated_at = ?
             WHERE scope_key = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(scope_key)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete_session(&self, scope_key: &str) -> ShepherdSessionResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM shepherd_sessions WHERE scope_key = ?")
            .bind(scope_key)
            .execute(pool)
            .await?;
        Ok(())
    }
}
