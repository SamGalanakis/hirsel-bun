//! Delta submission operations

use sqlx::Row;

use super::{DeltaState, DeltaStateError, DeltaStateResult};
use crate::core::db::utc_now;
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Delta Submission Operations
    // =========================================================================

    /// Create delta submissions from delta tasks
    pub async fn create_delta_submissions(
        &self,
        tasks: &[DeltaTask],
        batch_id: i64,
    ) -> DeltaStateResult<Vec<DeltaSubmission>> {
        let pool = self.pool().await?;
        let now = utc_now();
        let mut submissions = Vec::with_capacity(tasks.len());

        for task in tasks {
            let refs_json = serde_json::to_string(&task.refs)?;

            let result = sqlx::query(
                "INSERT INTO delta_submissions (project_id, route_id, batch_id, delta_type, draft_node_id, live_node_id, name, description, priority, status, refs, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
            )
            .bind(self.project_id)
            .bind(self.route_id)
            .bind(batch_id)
            .bind(task.delta_type.as_str())
            .bind(&task.draft_node_id)
            .bind(&task.live_node_id)
            .bind(&task.name)
            .bind(&task.description)
            .bind(task.priority)
            .bind(&refs_json)
            .bind(&now)
            .execute(pool)
            .await?;

            let id = result.last_insert_rowid();
            submissions.push(DeltaSubmission {
                id,
                project_id: self.project_id,
                batch_id: Some(batch_id),
                delta_type: task.delta_type,
                draft_node_id: task.draft_node_id.clone(),
                live_node_id: task.live_node_id.clone(),
                name: task.name.clone(),
                description: task.description.clone(),
                priority: task.priority,
                status: DeltaStatus::Pending,
                refs: task.refs.clone(),
                created_at: now.clone(),
                processed_at: None,
            });
        }

        Ok(submissions)
    }

    /// Get pending delta submissions for a batch
    pub async fn get_pending_submissions(
        &self,
        batch_id: i64,
    ) -> DeltaStateResult<Vec<DeltaSubmission>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, batch_id, delta_type, draft_node_id, live_node_id, name, description, priority, status, refs, created_at, processed_at
             FROM delta_submissions
             WHERE project_id = ? AND route_id = ? AND batch_id = ? AND status = 'pending'
             ORDER BY priority DESC, id",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(batch_id)
        .fetch_all(pool)
        .await?;

        let submissions = rows
            .into_iter()
            .map(|row| self.row_to_delta_submission(&row))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(submissions)
    }

    /// Update delta submission status
    pub async fn update_submission_status(
        &self,
        id: i64,
        status: DeltaStatus,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        let processed_at = if status == DeltaStatus::Done || status == DeltaStatus::Failed {
            Some(now)
        } else {
            None
        };

        sqlx::query("UPDATE delta_submissions SET status = ?, processed_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(processed_at)
            .bind(id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Get next batch ID
    pub async fn next_batch_id(&self) -> DeltaStateResult<i64> {
        let pool = self.pool().await?;
        let max: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(batch_id) FROM delta_submissions WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        Ok(max.unwrap_or(0) + 1)
    }

    pub(crate) fn row_to_delta_submission(
        &self,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<DeltaSubmission, DeltaStateError> {
        let submission_id: i64 = row.get("id");
        let refs_json: String = row.get("refs");
        let refs: Vec<Reference> = match serde_json::from_str(&refs_json) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(
                    "Failed to parse refs JSON for submission {}: {} - using empty",
                    submission_id,
                    e
                );
                Vec::new()
            }
        };
        let delta_type_str: String = row.get("delta_type");
        let delta_type = match DeltaType::from_str(&delta_type_str) {
            Some(dt) => dt,
            None => {
                tracing::debug!(
                    "Unknown delta_type '{}' for submission {} - using Implement",
                    delta_type_str,
                    submission_id
                );
                DeltaType::Implement
            }
        };
        let status_str: String = row.get("status");
        let status = DeltaStatus::from_str(&status_str);

        Ok(DeltaSubmission {
            id: submission_id,
            project_id: row.get("project_id"),
            batch_id: row.get("batch_id"),
            delta_type,
            draft_node_id: row.get("draft_node_id"),
            live_node_id: row.get("live_node_id"),
            name: row.get("name"),
            description: row.get("description"),
            priority: row.get("priority"),
            status,
            refs,
            created_at: row.get("created_at"),
            processed_at: row.get("processed_at"),
        })
    }
}
