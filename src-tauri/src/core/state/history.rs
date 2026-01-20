//! History and amendment methods
//!
//! Methods for managing history log and spec amendments.

use rusqlite::params;

use super::types::{Amendment, HistoryEntry, StateResult};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // History Methods
    // =========================================================================

    /// Get history entries
    pub fn get_history(&self, limit: i64) -> StateResult<Vec<HistoryEntry>> {
        let mut stmt = self.db.prepare(
            "SELECT id, timestamp, action, detail FROM history ORDER BY id DESC LIMIT ?1",
        )?;
        let entries: Vec<HistoryEntry> = stmt
            .query_map(params![limit], |row| {
                Ok(HistoryEntry {
                    id: row.get("id")?,
                    timestamp: row.get("timestamp")?,
                    action: row.get("action")?,
                    detail: row.get("detail")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    // =========================================================================
    // Amendment Methods
    // =========================================================================

    /// Add an amendment
    pub fn add_amendment(&self, message: &str, spec_hash: &str, author: &str) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO amendments (message, timestamp, author, spec_hash) VALUES (?1, ?2, ?3, ?4)",
            params![message, self.now(), author, spec_hash],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get all amendments
    pub fn get_amendments(&self) -> StateResult<Vec<Amendment>> {
        let mut stmt = self.db.prepare(
            "SELECT id, message, timestamp, author, spec_hash FROM amendments ORDER BY id",
        )?;
        let amendments: Vec<Amendment> = stmt
            .query_map([], |row| {
                Ok(Amendment {
                    id: row.get("id")?,
                    message: row.get("message")?,
                    timestamp: row.get("timestamp")?,
                    author: row.get("author")?,
                    spec_hash: row.get("spec_hash")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(amendments)
    }

    /// Get last spec hash
    pub fn get_last_spec_hash(&self) -> StateResult<Option<String>> {
        let result: Option<String> = self
            .db
            .query_row(
                "SELECT spec_hash FROM amendments ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(result)
    }
}
