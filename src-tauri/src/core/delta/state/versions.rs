//! Board version operations

use sqlx::Row;

use super::{DeltaState, DeltaStateError, DeltaStateResult};
use crate::core::db::utc_now;
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Board Version Operations
    // =========================================================================

    /// Create a new board version for this project
    pub async fn create_board_version(
        &self,
        batch_id: i64,
        description: Option<&str>,
    ) -> DeltaStateResult<BoardVersion> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Get next version number for this project
        let version_number: i32 = {
            let max: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(version_number) FROM board_versions WHERE project_id = ? AND route_id = ?",
            )
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_one(pool)
            .await?;
            max.unwrap_or(0) + 1
        };

        let result = sqlx::query(
            "INSERT INTO board_versions (project_id, route_id, batch_id, version_number, created_at, description)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(batch_id)
        .bind(version_number)
        .bind(&now)
        .bind(description)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(BoardVersion {
            id,
            project_id: self.project_id,
            batch_id,
            version_number,
            created_at: now,
            description: description.map(|s| s.to_string()),
        })
    }

    /// Get all board versions for this project
    pub async fn get_board_versions(&self) -> DeltaStateResult<Vec<BoardVersion>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE project_id = ? AND route_id = ?
             ORDER BY version_number DESC",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        let versions = rows
            .into_iter()
            .map(|row| self.row_to_board_version(&row))
            .collect();

        Ok(versions)
    }

    /// Get the latest board version for this project
    pub async fn get_latest_version(&self) -> DeltaStateResult<Option<BoardVersion>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE project_id = ? AND route_id = ?
             ORDER BY version_number DESC
             LIMIT 1",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| self.row_to_board_version(&row)))
    }

    /// Get a board version by ID
    pub async fn get_board_version(&self, id: i64) -> DeltaStateResult<BoardVersion> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, batch_id, version_number, created_at, description
             FROM board_versions
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::DraftNodeNotFound(format!("Board version {}", id)))?;

        Ok(self.row_to_board_version(&row))
    }

    pub(crate) fn row_to_board_version(&self, row: &sqlx::sqlite::SqliteRow) -> BoardVersion {
        BoardVersion {
            id: row.get("id"),
            project_id: row.get("project_id"),
            batch_id: row.get("batch_id"),
            version_number: row.get("version_number"),
            created_at: row.get("created_at"),
            description: row.get("description"),
        }
    }
}
