//! Structured worker-to-orchestrator concerns and reports.
//!
//! This replaces the old project-level worker messaging model with a narrower
//! coordination surface. Workers raise concerns or report progress upward;
//! the orchestrator and UI consume those structured events.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::OnceCell;

use super::db::{global_pool, utc_now};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS worker_concerns (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    runtime_name TEXT,
    worker_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    summary TEXT NOT NULL,
    details TEXT,
    status TEXT NOT NULL,
    source TEXT,
    resolution TEXT,
    resolved_by TEXT,
    resolved_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_worker_concerns_route
    ON worker_concerns(project_id, route_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_worker_concerns_status
    ON worker_concerns(status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_worker_concerns_worker
    ON worker_concerns(worker_name, updated_at DESC);

CREATE TABLE IF NOT EXISTS worker_concern_reads (
    reader TEXT NOT NULL,
    concern_id INTEGER NOT NULL,
    read_at TEXT NOT NULL,
    PRIMARY KEY (reader, concern_id)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerConcern {
    pub id: i64,
    pub project_id: i64,
    pub route_id: i64,
    pub runtime_name: Option<String>,
    pub worker_name: String,
    pub kind: String,
    pub severity: String,
    pub summary: String,
    pub details: Option<String>,
    pub status: String,
    pub source: Option<String>,
    pub resolution: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct CreateWorkerConcernRequest {
    pub project_id: i64,
    pub route_id: i64,
    pub runtime_name: Option<String>,
    pub worker_name: String,
    pub kind: String,
    pub severity: String,
    pub summary: String,
    pub details: Option<String>,
    pub status: String,
    pub source: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerConcernError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type WorkerConcernResult<T> = Result<T, WorkerConcernError>;

pub struct WorkerConcernStore;

impl WorkerConcernStore {
    pub async fn open() -> WorkerConcernResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    pub async fn create(
        &self,
        req: &CreateWorkerConcernRequest,
    ) -> WorkerConcernResult<WorkerConcern> {
        let pool = self.pool().await;
        let now = utc_now();

        let result = sqlx::query(
            r#"
            INSERT INTO worker_concerns (
                project_id,
                route_id,
                runtime_name,
                worker_name,
                kind,
                severity,
                summary,
                details,
                status,
                source,
                created_at,
                updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(req.project_id)
        .bind(req.route_id)
        .bind(&req.runtime_name)
        .bind(&req.worker_name)
        .bind(&req.kind)
        .bind(&req.severity)
        .bind(&req.summary)
        .bind(&req.details)
        .bind(&req.status)
        .bind(&req.source)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        self.get(result.last_insert_rowid()).await
    }

    pub async fn get(&self, id: i64) -> WorkerConcernResult<WorkerConcern> {
        let pool = self.pool().await;
        let row = sqlx::query(
            r#"
            SELECT id, project_id, route_id, runtime_name, worker_name, kind, severity,
                   summary, details, status, source, resolution, resolved_by,
                   resolved_at, created_at, updated_at
            FROM worker_concerns
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(Self::row_to_concern(&row))
    }

    pub async fn list_route(
        &self,
        project_id: i64,
        route_id: i64,
        include_resolved: bool,
        limit: Option<i64>,
    ) -> WorkerConcernResult<Vec<WorkerConcern>> {
        let pool = self.pool().await;
        let limit = limit.unwrap_or(100);
        let rows = if include_resolved {
            sqlx::query(
                r#"
                SELECT id, project_id, route_id, runtime_name, worker_name, kind, severity,
                       summary, details, status, source, resolution, resolved_by,
                       resolved_at, created_at, updated_at
                FROM worker_concerns
                WHERE project_id = ? AND route_id = ?
                ORDER BY updated_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(project_id)
            .bind(route_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, project_id, route_id, runtime_name, worker_name, kind, severity,
                       summary, details, status, source, resolution, resolved_by,
                       resolved_at, created_at, updated_at
                FROM worker_concerns
                WHERE project_id = ? AND route_id = ? AND status != 'resolved'
                ORDER BY updated_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(project_id)
            .bind(route_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_concern(&row))
            .collect())
    }

    pub async fn list_unread(
        &self,
        reader: &str,
        limit: Option<i64>,
    ) -> WorkerConcernResult<Vec<WorkerConcern>> {
        let pool = self.pool().await;
        let limit = limit.unwrap_or(100);
        let rows = sqlx::query(
            r#"
            SELECT c.id, c.project_id, c.route_id, c.runtime_name, c.worker_name, c.kind,
                   c.severity, c.summary, c.details, c.status, c.source, c.resolution,
                   c.resolved_by, c.resolved_at, c.created_at, c.updated_at
            FROM worker_concerns c
            LEFT JOIN worker_concern_reads r
              ON r.concern_id = c.id AND r.reader = ?
            WHERE r.concern_id IS NULL
              AND c.status != 'resolved'
            ORDER BY c.updated_at DESC, c.id DESC
            LIMIT ?
            "#,
        )
        .bind(reader)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_concern(&row))
            .collect())
    }

    pub async fn mark_read(&self, concern_id: i64, reader: &str) -> WorkerConcernResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "INSERT OR REPLACE INTO worker_concern_reads (reader, concern_id, read_at) VALUES (?, ?, ?)",
        )
        .bind(reader)
        .bind(concern_id)
        .bind(utc_now())
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_route_read(
        &self,
        project_id: i64,
        route_id: i64,
        reader: &str,
    ) -> WorkerConcernResult<()> {
        let pool = self.pool().await;
        let concern_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM worker_concerns WHERE project_id = ? AND route_id = ?",
        )
        .bind(project_id)
        .bind(route_id)
        .fetch_all(pool)
        .await?;

        for concern_id in concern_ids {
            self.mark_read(concern_id, reader).await?;
        }

        Ok(())
    }

    pub async fn resolve(
        &self,
        concern_id: i64,
        resolver: &str,
        resolution: Option<&str>,
    ) -> WorkerConcernResult<WorkerConcern> {
        let pool = self.pool().await;
        let now = utc_now();
        sqlx::query(
            r#"
            UPDATE worker_concerns
            SET status = 'resolved',
                resolution = ?,
                resolved_by = ?,
                resolved_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(resolution)
        .bind(resolver)
        .bind(&now)
        .bind(&now)
        .bind(concern_id)
        .execute(pool)
        .await?;

        self.get(concern_id).await
    }

    pub async fn delete_project_concerns(&self, project_id: i64) -> WorkerConcernResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "DELETE FROM worker_concern_reads WHERE concern_id IN (SELECT id FROM worker_concerns WHERE project_id = ?)",
        )
        .bind(project_id)
        .execute(pool)
        .await?;
        sqlx::query("DELETE FROM worker_concerns WHERE project_id = ?")
            .bind(project_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_route_concerns(&self, route_id: i64) -> WorkerConcernResult<()> {
        let pool = self.pool().await;
        sqlx::query(
            "DELETE FROM worker_concern_reads WHERE concern_id IN (SELECT id FROM worker_concerns WHERE route_id = ?)",
        )
        .bind(route_id)
        .execute(pool)
        .await?;
        sqlx::query("DELETE FROM worker_concerns WHERE route_id = ?")
            .bind(route_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    fn row_to_concern(row: &sqlx::sqlite::SqliteRow) -> WorkerConcern {
        WorkerConcern {
            id: row.get("id"),
            project_id: row.get("project_id"),
            route_id: row.get("route_id"),
            runtime_name: row.get("runtime_name"),
            worker_name: row.get("worker_name"),
            kind: row.get("kind"),
            severity: row.get("severity"),
            summary: row.get("summary"),
            details: row.get("details"),
            status: row.get("status"),
            source: row.get("source"),
            resolution: row.get("resolution"),
            resolved_by: row.get("resolved_by"),
            resolved_at: row.get("resolved_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }
}
