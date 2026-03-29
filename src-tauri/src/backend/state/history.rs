//! History methods
//!
//! Methods for managing history log.

use sqlx::Row;

use super::types::{HistoryEntry, StateResult};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // History Methods
    // =========================================================================

    /// Get history entries
    pub async fn get_history(&self, limit: i64) -> StateResult<Vec<HistoryEntry>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, timestamp, action, detail FROM history ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&pool)
        .await?;

        let entries = rows
            .into_iter()
            .map(|row| HistoryEntry {
                id: row.get("id"),
                timestamp: row.get("timestamp"),
                action: row.get("action"),
                detail: row.get("detail"),
            })
            .collect();
        Ok(entries)
    }
}
