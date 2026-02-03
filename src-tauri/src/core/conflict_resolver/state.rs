//! State management for conflict resolutions
//!
//! Tracks conflict resolution attempts in the database.

use sqlx::{Row, SqlitePool};
use thiserror::Error;
use tokio::sync::OnceCell;

use crate::core::db::{global_pool, utc_now};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS conflict_resolutions (
    id INTEGER PRIMARY KEY,
    delivery_id INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    UNIQUE(delivery_id, file_path)
);
CREATE INDEX IF NOT EXISTS idx_conflict_resolutions_delivery ON conflict_resolutions(delivery_id);
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

/// Error type for state operations
#[derive(Error, Debug)]
pub enum StateError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Status of a conflict resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolutionStatus {
    Pending,
    InProgress,
    Resolved,
    Failed,
}

impl ConflictResolutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Resolved => "resolved",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "resolved" => Self::Resolved,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// A conflict resolution record
#[derive(Debug, Clone)]
pub struct ConflictResolution {
    pub id: i64,
    pub delivery_id: i64,
    pub file_path: String,
    pub status: ConflictResolutionStatus,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

/// State manager for conflict resolutions
pub struct ConflictResolverState {
    delivery_id: i64,
}

impl ConflictResolverState {
    /// Create a new state manager for a delivery
    pub fn new(delivery_id: i64) -> Self {
        Self { delivery_id }
    }

    /// Get the pool and ensure schema
    async fn pool(&self) -> Result<&'static SqlitePool, StateError> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(pool)
    }

    /// Create resolution records for a set of conflicting files
    pub async fn create_resolutions(
        &self,
        files: &[String],
    ) -> Result<Vec<ConflictResolution>, StateError> {
        let pool = self.pool().await?;
        let now = utc_now();
        let mut resolutions = Vec::with_capacity(files.len());

        for file_path in files {
            let result = sqlx::query(
                "INSERT OR REPLACE INTO conflict_resolutions (delivery_id, file_path, status, created_at)
                 VALUES (?, ?, 'pending', ?)",
            )
            .bind(self.delivery_id)
            .bind(file_path)
            .bind(&now)
            .execute(pool)
            .await?;

            let id = result.last_insert_rowid();
            resolutions.push(ConflictResolution {
                id,
                delivery_id: self.delivery_id,
                file_path: file_path.clone(),
                status: ConflictResolutionStatus::Pending,
                created_at: now.clone(),
                resolved_at: None,
            });
        }

        Ok(resolutions)
    }

    /// Get all resolutions for this delivery
    pub async fn get_resolutions(&self) -> Result<Vec<ConflictResolution>, StateError> {
        let pool = self.pool().await?;

        let rows = sqlx::query(
            "SELECT id, delivery_id, file_path, status, created_at, resolved_at
             FROM conflict_resolutions
             WHERE delivery_id = ?
             ORDER BY id",
        )
        .bind(self.delivery_id)
        .fetch_all(pool)
        .await?;

        let resolutions = rows
            .into_iter()
            .map(|row| ConflictResolution {
                id: row.get("id"),
                delivery_id: row.get("delivery_id"),
                file_path: row.get("file_path"),
                status: ConflictResolutionStatus::from_str(&row.get::<String, _>("status")),
                created_at: row.get("created_at"),
                resolved_at: row.get("resolved_at"),
            })
            .collect();

        Ok(resolutions)
    }

    /// Update resolution status
    pub async fn update_status(
        &self,
        id: i64,
        status: ConflictResolutionStatus,
    ) -> Result<(), StateError> {
        let pool = self.pool().await?;
        let now = utc_now();

        let resolved_at = if status == ConflictResolutionStatus::Resolved {
            Some(now.clone())
        } else {
            None
        };

        sqlx::query("UPDATE conflict_resolutions SET status = ?, resolved_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(resolved_at)
            .bind(id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Mark all resolutions as complete
    pub async fn mark_all_resolved(&self) -> Result<(), StateError> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE conflict_resolutions SET status = 'resolved', resolved_at = ? WHERE delivery_id = ?",
        )
        .bind(&now)
        .bind(self.delivery_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark all resolutions as failed
    pub async fn mark_all_failed(&self) -> Result<(), StateError> {
        let pool = self.pool().await?;

        sqlx::query(
            "UPDATE conflict_resolutions SET status = 'failed' WHERE delivery_id = ? AND status != 'resolved'",
        )
        .bind(self.delivery_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Check if all resolutions are complete
    pub async fn all_resolved(&self) -> Result<bool, StateError> {
        let pool = self.pool().await?;

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM conflict_resolutions WHERE delivery_id = ? AND status != 'resolved'",
        )
        .bind(self.delivery_id)
        .fetch_one(pool)
        .await?;

        Ok(count == 0)
    }
}
