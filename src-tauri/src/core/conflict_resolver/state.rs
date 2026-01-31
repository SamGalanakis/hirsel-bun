//! State management for conflict resolutions
//!
//! Tracks conflict resolution attempts in the database.

use rusqlite::{params, Connection};
use thiserror::Error;

use crate::core::config::global_db_path;

/// Error type for state operations
#[derive(Error, Debug)]
pub enum StateError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
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

    /// Open database connection and ensure schema exists
    fn open_db(&self) -> Result<Connection, StateError> {
        let db = Connection::open(global_db_path())?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        self.ensure_schema(&db)?;
        Ok(db)
    }

    /// Ensure the conflict_resolutions table exists
    fn ensure_schema(&self, db: &Connection) -> Result<(), StateError> {
        db.execute(
            r#"
            CREATE TABLE IF NOT EXISTS conflict_resolutions (
                id INTEGER PRIMARY KEY,
                delivery_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                resolved_at TEXT,
                UNIQUE(delivery_id, file_path)
            )
            "#,
            [],
        )?;
        db.execute(
            "CREATE INDEX IF NOT EXISTS idx_conflict_resolutions_delivery ON conflict_resolutions(delivery_id)",
            [],
        )?;
        Ok(())
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string()
    }

    /// Create resolution records for a set of conflicting files
    pub fn create_resolutions(
        &self,
        files: &[String],
    ) -> Result<Vec<ConflictResolution>, StateError> {
        let db = self.open_db()?;
        let now = self.now();
        let mut resolutions = Vec::with_capacity(files.len());

        for file_path in files {
            db.execute(
                "INSERT OR REPLACE INTO conflict_resolutions (delivery_id, file_path, status, created_at)
                 VALUES (?1, ?2, 'pending', ?3)",
                params![self.delivery_id, file_path, &now],
            )?;

            let id = db.last_insert_rowid();
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
    pub fn get_resolutions(&self) -> Result<Vec<ConflictResolution>, StateError> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, delivery_id, file_path, status, created_at, resolved_at
             FROM conflict_resolutions
             WHERE delivery_id = ?1
             ORDER BY id",
        )?;

        let resolutions = stmt
            .query_map([self.delivery_id], |row| {
                Ok(ConflictResolution {
                    id: row.get("id")?,
                    delivery_id: row.get("delivery_id")?,
                    file_path: row.get("file_path")?,
                    status: ConflictResolutionStatus::from_str(
                        &row.get::<_, String>("status").unwrap_or_default(),
                    ),
                    created_at: row.get("created_at")?,
                    resolved_at: row.get("resolved_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(resolutions)
    }

    /// Update resolution status
    pub fn update_status(
        &self,
        id: i64,
        status: ConflictResolutionStatus,
    ) -> Result<(), StateError> {
        let db = self.open_db()?;
        let now = self.now();

        let resolved_at = if status == ConflictResolutionStatus::Resolved {
            Some(now.clone())
        } else {
            None
        };

        db.execute(
            "UPDATE conflict_resolutions SET status = ?1, resolved_at = ?2 WHERE id = ?3",
            params![status.as_str(), resolved_at, id],
        )?;

        Ok(())
    }

    /// Mark all resolutions as complete
    pub fn mark_all_resolved(&self) -> Result<(), StateError> {
        let db = self.open_db()?;
        let now = self.now();

        db.execute(
            "UPDATE conflict_resolutions SET status = 'resolved', resolved_at = ?1 WHERE delivery_id = ?2",
            params![&now, self.delivery_id],
        )?;

        Ok(())
    }

    /// Mark all resolutions as failed
    pub fn mark_all_failed(&self) -> Result<(), StateError> {
        let db = self.open_db()?;

        db.execute(
            "UPDATE conflict_resolutions SET status = 'failed' WHERE delivery_id = ?1 AND status != 'resolved'",
            params![self.delivery_id],
        )?;

        Ok(())
    }

    /// Check if all resolutions are complete
    pub fn all_resolved(&self) -> Result<bool, StateError> {
        let db = self.open_db()?;

        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM conflict_resolutions WHERE delivery_id = ?1 AND status != 'resolved'",
            [self.delivery_id],
            |row| row.get(0),
        )?;

        Ok(count == 0)
    }
}
