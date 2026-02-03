//! Worker event methods
//!
//! Methods for managing real-time worker output events.

use sqlx::Row;

use super::types::{StateResult, ToolCallStatus, WorkerEvent, WorkerEventType};
use super::SQLiteState;
use crate::core::db::utc_now;

impl SQLiteState {
    // =========================================================================
    // Worker Event Methods
    // =========================================================================

    /// Insert a text output event
    pub async fn insert_text_event(&self, worker_name: &str, content: &str) -> StateResult<i64> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, content)
             VALUES (?, ?, ?, ?)",
        )
        .bind(worker_name)
        .bind("text")
        .bind(utc_now())
        .bind(content)
        .execute(&pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Insert a thought event
    pub async fn insert_thought_event(&self, worker_name: &str, content: &str) -> StateResult<i64> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, content)
             VALUES (?, ?, ?, ?)",
        )
        .bind(worker_name)
        .bind("thought")
        .bind(utc_now())
        .bind(content)
        .execute(&pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Insert a tool start event, or update if tool_call_id already exists with better data
    pub async fn insert_tool_start_event(
        &self,
        worker_name: &str,
        tool_call_id: &str,
        title: &str,
        kind: Option<&str>,
        status: ToolCallStatus,
        input: Option<&str>,
    ) -> StateResult<i64> {
        let pool = self.pool().await;

        // Check if this tool_call_id already exists
        let existing: Option<(i64, Option<String>)> = sqlx::query(
            "SELECT id, tool_input FROM worker_events WHERE tool_call_id = ? AND event_type = 'tool_start'",
        )
        .bind(tool_call_id)
        .fetch_optional(&pool)
        .await?
        .map(|row| (row.get("id"), row.get("tool_input")));

        if let Some((existing_id, existing_input)) = existing {
            // Update if new data has input and existing doesn't (second event has actual command)
            let has_better_input = input.is_some()
                && input != Some("{}")
                && (existing_input.is_none() || existing_input.as_deref() == Some("{}"));

            if has_better_input || title != "Terminal" {
                sqlx::query(
                    "UPDATE worker_events SET tool_title = ?, tool_kind = ?, tool_status = ?, tool_input = ?
                     WHERE id = ?",
                )
                .bind(title)
                .bind(kind)
                .bind(status.as_str())
                .bind(input)
                .bind(existing_id)
                .execute(&pool)
                .await?;
            }
            return Ok(existing_id);
        }

        // Insert new row
        let result = sqlx::query(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, tool_call_id, tool_title, tool_kind, tool_status, tool_input)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(worker_name)
        .bind("tool_start")
        .bind(utc_now())
        .bind(tool_call_id)
        .bind(title)
        .bind(kind)
        .bind(status.as_str())
        .bind(input)
        .execute(&pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Insert a tool update event
    pub async fn insert_tool_update_event(
        &self,
        worker_name: &str,
        tool_call_id: &str,
        title: Option<&str>,
        status: Option<ToolCallStatus>,
        output: Option<&str>,
    ) -> StateResult<i64> {
        let pool = self.pool().await;
        let result = sqlx::query(
            "INSERT INTO worker_events (worker_name, event_type, timestamp, tool_call_id, tool_title, tool_status, tool_output)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(worker_name)
        .bind("tool_update")
        .bind(utc_now())
        .bind(tool_call_id)
        .bind(title)
        .bind(status.map(|s| s.as_str()))
        .bind(output)
        .execute(&pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Get worker events since a given ID (for polling)
    pub async fn get_worker_events(
        &self,
        worker_name: &str,
        after_id: Option<i64>,
        limit: i64,
    ) -> StateResult<Vec<WorkerEvent>> {
        let pool = self.pool().await;
        let after = after_id.unwrap_or(0);
        let rows = sqlx::query(
            "SELECT id, worker_name, event_type, timestamp, content,
                    tool_call_id, tool_title, tool_kind, tool_status, tool_input, tool_output
             FROM worker_events
             WHERE worker_name = ? AND id > ?
             ORDER BY id ASC
             LIMIT ?",
        )
        .bind(worker_name)
        .bind(after)
        .bind(limit)
        .fetch_all(&pool)
        .await?;

        let events = rows
            .into_iter()
            .map(|row| {
                let event_type_str: String = row.get("event_type");
                let tool_status_str: Option<String> = row.get("tool_status");

                WorkerEvent {
                    id: row.get("id"),
                    worker_name: row.get("worker_name"),
                    event_type: WorkerEventType::from_str(&event_type_str)
                        .unwrap_or(WorkerEventType::Text),
                    timestamp: row.get("timestamp"),
                    content: row.get("content"),
                    tool_call_id: row.get("tool_call_id"),
                    tool_title: row.get("tool_title"),
                    tool_kind: row.get("tool_kind"),
                    tool_status: tool_status_str.and_then(|s| ToolCallStatus::from_str(&s)),
                    tool_input: row.get("tool_input"),
                    tool_output: row.get("tool_output"),
                }
            })
            .collect();

        Ok(events)
    }

    /// Get all events for a worker (for initial load)
    pub async fn get_all_worker_events(
        &self,
        worker_name: &str,
        limit: i64,
    ) -> StateResult<Vec<WorkerEvent>> {
        self.get_worker_events(worker_name, None, limit).await
    }

    /// Clear old worker events (for cleanup)
    pub async fn clear_worker_events(&self, worker_name: &str) -> StateResult<()> {
        let pool = self.pool().await;
        sqlx::query("DELETE FROM worker_events WHERE worker_name = ?")
            .bind(worker_name)
            .execute(&pool)
            .await?;
        Ok(())
    }
}
