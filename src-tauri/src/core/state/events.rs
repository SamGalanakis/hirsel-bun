//! Worker event methods
//!
//! Methods for managing real-time worker output events.

use rusqlite::params;

use super::types::{StateResult, ToolCallStatus, WorkerEvent, WorkerEventType};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // Worker Event Methods
    // =========================================================================

    /// Insert a text output event
    pub fn insert_text_event(&self, worker_name: &str, content: &str) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, content)
             VALUES (?1, ?2, ?3, ?4)",
            params![worker_name, "text", self.now(), content],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Insert a thought event
    pub fn insert_thought_event(&self, worker_name: &str, content: &str) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, content)
             VALUES (?1, ?2, ?3, ?4)",
            params![worker_name, "thought", self.now(), content],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Insert a tool start event, or update if tool_call_id already exists with better data
    pub fn insert_tool_start_event(
        &self,
        worker_name: &str,
        tool_call_id: &str,
        title: &str,
        kind: Option<&str>,
        status: ToolCallStatus,
        input: Option<&str>,
    ) -> StateResult<i64> {
        // Check if this tool_call_id already exists
        let existing: Option<(i64, Option<String>)> = self
            .db
            .query_row(
                "SELECT id, tool_input FROM worker_events WHERE tool_call_id = ?1 AND event_type = 'tool_start'",
                params![tool_call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((existing_id, existing_input)) = existing {
            // Update if new data has input and existing doesn't (second event has actual command)
            let has_better_input = input.is_some()
                && input != Some("{}")
                && (existing_input.is_none() || existing_input.as_deref() == Some("{}"));

            if has_better_input || title != "Terminal" {
                self.db.execute(
                    "UPDATE worker_events SET tool_title = ?1, tool_kind = ?2, tool_status = ?3, tool_input = ?4
                     WHERE id = ?5",
                    params![title, kind, status.as_str(), input, existing_id],
                )?;
            }
            return Ok(existing_id);
        }

        // Insert new row
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, tool_call_id, tool_title, tool_kind, tool_status, tool_input)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                worker_name,
                "tool_start",
                self.now(),
                tool_call_id,
                title,
                kind,
                status.as_str(),
                input
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Insert a tool update event
    pub fn insert_tool_update_event(
        &self,
        worker_name: &str,
        tool_call_id: &str,
        title: Option<&str>,
        status: Option<ToolCallStatus>,
        output: Option<&str>,
    ) -> StateResult<i64> {
        self.db.execute(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, tool_call_id, tool_title, tool_status, tool_output)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                worker_name,
                "tool_update",
                self.now(),
                tool_call_id,
                title,
                status.map(|s| s.as_str()),
                output
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Get worker events since a given ID (for polling)
    pub fn get_worker_events(
        &self,
        worker_name: &str,
        after_id: Option<i64>,
        limit: i64,
    ) -> StateResult<Vec<WorkerEvent>> {
        let after = after_id.unwrap_or(0);
        let mut stmt = self.db.prepare(
            "SELECT id, worker_name, event_type, timestamp, content,
                    tool_call_id, tool_title, tool_kind, tool_status, tool_input, tool_output
             FROM worker_events
             WHERE worker_name = ?1 AND id > ?2
             ORDER BY id ASC
             LIMIT ?3",
        )?;

        let events: Vec<WorkerEvent> = stmt
            .query_map(params![worker_name, after, limit], |row| {
                let event_type_str: String = row.get("event_type")?;
                let tool_status_str: Option<String> = row.get("tool_status")?;

                Ok(WorkerEvent {
                    id: row.get("id")?,
                    worker_name: row.get("worker_name")?,
                    event_type: WorkerEventType::from_str(&event_type_str)
                        .unwrap_or(WorkerEventType::Text),
                    timestamp: row.get("timestamp")?,
                    content: row.get("content")?,
                    tool_call_id: row.get("tool_call_id")?,
                    tool_title: row.get("tool_title")?,
                    tool_kind: row.get("tool_kind")?,
                    tool_status: tool_status_str.and_then(|s| ToolCallStatus::from_str(&s)),
                    tool_input: row.get("tool_input")?,
                    tool_output: row.get("tool_output")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    /// Get all events for a worker (for initial load)
    pub fn get_all_worker_events(
        &self,
        worker_name: &str,
        limit: i64,
    ) -> StateResult<Vec<WorkerEvent>> {
        self.get_worker_events(worker_name, None, limit)
    }

    /// Clear old worker events (for cleanup)
    pub fn clear_worker_events(&self, worker_name: &str) -> StateResult<()> {
        self.db.execute(
            "DELETE FROM worker_events WHERE worker_name = ?1",
            params![worker_name],
        )?;
        Ok(())
    }
}
