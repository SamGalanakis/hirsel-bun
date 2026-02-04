//! Delivery operations

use sqlx::Row;

use super::{ensure_schema, DeltaState, DeltaStateResult};
use crate::core::db::{global_pool, utc_now};
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Delivery Operations
    // =========================================================================

    /// Create a new delivery for a board version
    pub async fn create_delivery(
        &self,
        version_id: i64,
        target_branch: &str,
    ) -> DeltaStateResult<Delivery> {
        let pool = self.pool().await?;

        let result = sqlx::query(
            "INSERT INTO deliveries (project_id, route_id, version_id, status, target_branch)
             VALUES (?, ?, ?, 'pending', ?)",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(version_id)
        .bind(target_branch)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(Delivery {
            id,
            project_id: self.project_id,
            route_id: self.route_id,
            version_id,
            status: BoardDeliveryStatus::Pending,
            target_branch: target_branch.to_string(),
            delivery_branch: None,
            pr_url: None,
            pr_number: None,
            started_at: None,
            completed_at: None,
            failure_reason: None,
        })
    }

    /// Get the current (non-terminal) delivery for this project
    pub async fn get_current_delivery(&self) -> DeltaStateResult<Option<Delivery>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, route_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE project_id = ? AND route_id = ? AND status NOT IN ('merged', 'abandoned', 'failed')
             ORDER BY id DESC
             LIMIT 1",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| self.row_to_delivery(&row)))
    }

    /// Get a delivery by ID
    pub async fn get_delivery(&self, id: i64) -> DeltaStateResult<Delivery> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, route_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| super::DeltaStateError::DraftNodeNotFound(format!("Delivery {}", id)))?;

        Ok(self.row_to_delivery(&row))
    }

    /// Get all deliveries for a board version
    pub async fn get_deliveries_for_version(
        &self,
        version_id: i64,
    ) -> DeltaStateResult<Vec<Delivery>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, route_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE version_id = ?
             ORDER BY id DESC",
        )
        .bind(version_id)
        .fetch_all(pool)
        .await?;

        let deliveries = rows
            .into_iter()
            .map(|row| self.row_to_delivery(&row))
            .collect();

        Ok(deliveries)
    }

    /// Update delivery status
    pub async fn update_delivery_status(
        &self,
        id: i64,
        status: BoardDeliveryStatus,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Build dynamic query based on status
        let sql = if status == BoardDeliveryStatus::InProgress {
            "UPDATE deliveries SET status = ?, started_at = COALESCE(started_at, ?) WHERE id = ?"
        } else if status.is_terminal() {
            "UPDATE deliveries SET status = ?, completed_at = ? WHERE id = ?"
        } else {
            "UPDATE deliveries SET status = ? WHERE id = ?"
        };

        if status == BoardDeliveryStatus::InProgress || status.is_terminal() {
            sqlx::query(sql)
                .bind(status.as_str())
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?;
        } else {
            sqlx::query(sql)
                .bind(status.as_str())
                .bind(id)
                .execute(pool)
                .await?;
        }

        Ok(())
    }

    /// Update delivery with branch and PR info
    pub async fn update_delivery_info(
        &self,
        id: i64,
        delivery_branch: Option<&str>,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query(
            "UPDATE deliveries SET delivery_branch = ?, pr_url = ?, pr_number = ? WHERE id = ?",
        )
        .bind(delivery_branch)
        .bind(pr_url)
        .bind(pr_number)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark delivery as failed with a reason
    pub async fn fail_delivery(&self, id: i64, reason: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE deliveries SET status = 'failed', completed_at = ?, failure_reason = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub(crate) fn row_to_delivery(&self, row: &sqlx::sqlite::SqliteRow) -> Delivery {
        Delivery {
            id: row.get("id"),
            project_id: row.get("project_id"),
            route_id: row.get("route_id"),
            version_id: row.get("version_id"),
            status: BoardDeliveryStatus::from_str(&row.get::<String, _>("status")),
            target_branch: row.get("target_branch"),
            delivery_branch: row.get("delivery_branch"),
            pr_url: row.get("pr_url"),
            pr_number: row.get("pr_number"),
            started_at: row.get("started_at"),
            completed_at: row.get("completed_at"),
            failure_reason: row.get("failure_reason"),
        }
    }

    // =========================================================================
    // Delivery Attempt Operations
    // =========================================================================

    /// Add a delivery attempt
    pub async fn add_delivery_attempt(
        &self,
        delivery_id: i64,
    ) -> DeltaStateResult<DeliveryAttempt> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Get next attempt number
        let attempt_number: i32 = {
            let max: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(attempt_number) FROM delivery_attempts WHERE delivery_id = ?",
            )
            .bind(delivery_id)
            .fetch_one(pool)
            .await?;
            max.unwrap_or(0) + 1
        };

        let result = sqlx::query(
            "INSERT INTO delivery_attempts (delivery_id, attempt_number, status, started_at)
             VALUES (?, ?, 'failed', ?)",
        )
        .bind(delivery_id)
        .bind(attempt_number)
        .bind(&now)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        Ok(DeliveryAttempt {
            id,
            delivery_id,
            attempt_number,
            status: DeliveryAttemptStatus::Failed, // Will be updated on completion
            started_at: now,
            completed_at: None,
            error_message: None,
        })
    }

    /// Complete a delivery attempt
    pub async fn complete_delivery_attempt(
        &self,
        id: i64,
        status: DeliveryAttemptStatus,
        error_message: Option<&str>,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE delivery_attempts SET status = ?, completed_at = ?, error_message = ? WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(&now)
        .bind(error_message)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get delivery attempts for a delivery
    pub async fn get_delivery_attempts(
        &self,
        delivery_id: i64,
    ) -> DeltaStateResult<Vec<DeliveryAttempt>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, delivery_id, attempt_number, status, started_at, completed_at, error_message
             FROM delivery_attempts
             WHERE delivery_id = ?
             ORDER BY attempt_number DESC",
        )
        .bind(delivery_id)
        .fetch_all(pool)
        .await?;

        let attempts = rows
            .into_iter()
            .map(|row| DeliveryAttempt {
                id: row.get("id"),
                delivery_id: row.get("delivery_id"),
                attempt_number: row.get("attempt_number"),
                status: DeliveryAttemptStatus::from_str(&row.get::<String, _>("status")),
                started_at: row.get("started_at"),
                completed_at: row.get("completed_at"),
                error_message: row.get("error_message"),
            })
            .collect();

        Ok(attempts)
    }

    /// List all deliveries in resolving_conflicts status (for daemon polling)
    pub async fn list_resolving_deliveries() -> DeltaStateResult<Vec<Delivery>> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, route_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE status = 'resolving_conflicts'",
        )
        .fetch_all(pool)
        .await?;

        let deliveries = rows
            .into_iter()
            .map(|row| Delivery {
                id: row.get("id"),
                project_id: row.get("project_id"),
                route_id: row.get("route_id"),
                version_id: row.get("version_id"),
                status: BoardDeliveryStatus::ResolvingConflicts,
                target_branch: row.get("target_branch"),
                delivery_branch: row.get("delivery_branch"),
                pr_url: row.get("pr_url"),
                pr_number: row.get("pr_number"),
                started_at: row.get("started_at"),
                completed_at: row.get("completed_at"),
                failure_reason: row.get("failure_reason"),
            })
            .collect();

        Ok(deliveries)
    }
}
