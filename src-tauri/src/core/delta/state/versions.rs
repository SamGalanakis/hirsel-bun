//! Board version operations

use sqlx::Row;

use super::{DeltaState, DeltaStateResult};
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Board Version Operations
    // =========================================================================

    /// Get all board versions for this project
    pub async fn get_board_versions(&self) -> DeltaStateResult<Vec<BoardVersion>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, version_number, created_at, description
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
            "SELECT id, project_id, version_number, created_at, description
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

    pub(crate) fn row_to_board_version(&self, row: &sqlx::sqlite::SqliteRow) -> BoardVersion {
        BoardVersion {
            id: row.get("id"),
            project_id: row.get("project_id"),
            version_number: row.get("version_number"),
            created_at: row.get("created_at"),
            description: row.get("description"),
        }
    }
}
