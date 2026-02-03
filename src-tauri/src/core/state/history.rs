//! History and amendment methods
//!
//! Methods for managing history log and spec amendments.

use sqlx::Row;

use super::types::{Amendment, HistoryEntry, StateResult};
use super::SQLiteState;
use crate::core::db::utc_now;

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

    // =========================================================================
    // Amendment Methods
    // =========================================================================

    /// Add an amendment
    pub async fn add_amendment(
        &self,
        message: &str,
        spec_hash: &str,
        author: &str,
    ) -> StateResult<i64> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "INSERT INTO amendments (message, timestamp, author, spec_hash) VALUES (?, ?, ?, ?)",
        )
        .bind(message)
        .bind(utc_now())
        .bind(author)
        .bind(spec_hash)
        .execute(&pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Get all amendments
    pub async fn get_amendments(&self) -> StateResult<Vec<Amendment>> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT id, message, timestamp, author, spec_hash FROM amendments ORDER BY id",
        )
        .fetch_all(&pool)
        .await?;

        let amendments = rows
            .into_iter()
            .map(|row| Amendment {
                id: row.get("id"),
                message: row.get("message"),
                timestamp: row.get("timestamp"),
                author: row.get("author"),
                spec_hash: row.get("spec_hash"),
            })
            .collect();
        Ok(amendments)
    }

    /// Get last spec hash
    pub async fn get_last_spec_hash(&self) -> StateResult<Option<String>> {
        let pool = self.pool().await;
        let result: Option<String> =
            sqlx::query_scalar("SELECT spec_hash FROM amendments ORDER BY id DESC LIMIT 1")
                .fetch_optional(&pool)
                .await?;
        Ok(result)
    }
}
