//! Route runtime persistence.

use sqlx::Row;

use super::{ensure_schema, DeltaState, DeltaStateResult};
use crate::backend::db::{global_pool, utc_now};
use crate::backend::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Route Runtime Operations
    // =========================================================================

    /// Get the persistent route runtime for this route.
    pub async fn get_route_runtime(&self) -> DeltaStateResult<Option<RouteRuntime>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, route_id, runtime_name, status, created_at, last_dispatch_at
             FROM route_runtimes
             WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| self.row_to_route_runtime(&row)))
    }

    /// Create a new persistent route runtime for this route.
    pub async fn create_route_runtime(&self, runtime_name: &str) -> DeltaStateResult<RouteRuntime> {
        let pool = self.pool().await?;
        let now = utc_now();

        let result = sqlx::query(
            "INSERT INTO route_runtimes (project_id, route_id, runtime_name, status, created_at)
             VALUES (?, ?, ?, 'paused', ?)",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(runtime_name)
        .bind(&now)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        self.bump_tree_generation().await?;
        Ok(RouteRuntime {
            id,
            project_id: self.project_id,
            route_id: self.route_id,
            runtime_name: runtime_name.to_string(),
            status: RouteRuntimeStatus::Paused,
            created_at: now,
            last_dispatch_at: None,
        })
    }

    /// Update the route runtime status.
    pub async fn update_route_runtime_status(
        &self,
        status: RouteRuntimeStatus,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query("UPDATE route_runtimes SET status = ? WHERE project_id = ? AND route_id = ?")
            .bind(status.as_str())
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    pub(crate) fn row_to_route_runtime(&self, row: &sqlx::sqlite::SqliteRow) -> RouteRuntime {
        let runtime_name: String = row.get("runtime_name");
        let status_str: String = row.get("status");
        let status = RouteRuntimeStatus::from_str(&status_str);

        RouteRuntime {
            id: row.get("id"),
            project_id: row.get("project_id"),
            route_id: row.get("route_id"),
            runtime_name,
            status,
            created_at: row.get("created_at"),
            last_dispatch_at: row.get("last_dispatch_at"),
        }
    }

    /// List all route runtimes across all projects.
    ///
    /// Returns tuples of (RouteRuntime, project_name) for building runtime summaries.
    pub async fn list_all_route_runtimes() -> DeltaStateResult<Vec<(RouteRuntime, String)>> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;

        let rows = sqlx::query(
            "SELECT pr.id, pr.project_id, pr.route_id, pr.runtime_name, pr.status, pr.created_at, pr.last_dispatch_at,
                    p.name as project_name
             FROM route_runtimes pr
             JOIN projects p ON pr.project_id = p.id
             ORDER BY pr.created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        let runs = rows
            .into_iter()
            .map(|row| {
                let status_str: String = row.get("status");
                (
                    RouteRuntime {
                        id: row.get("id"),
                        project_id: row.get("project_id"),
                        route_id: row.get("route_id"),
                        runtime_name: row.get("runtime_name"),
                        status: RouteRuntimeStatus::from_str(&status_str),
                        created_at: row.get("created_at"),
                        last_dispatch_at: row.get("last_dispatch_at"),
                    },
                    row.get::<String, _>("project_name"),
                )
            })
            .collect();

        Ok(runs)
    }
}

/// List all working route runtimes.
pub async fn list_working_route_runtimes() -> DeltaStateResult<Vec<(i64, i64, String)>> {
    let pool = global_pool().await;
    ensure_schema(pool).await?;

    let rows = sqlx::query(
        "SELECT project_id, route_id, runtime_name FROM route_runtimes WHERE status = 'working'",
    )
    .fetch_all(pool)
    .await?;

    let runs = rows
        .into_iter()
        .map(|row| {
            (
                row.get::<i64, _>("project_id"),
                row.get::<i64, _>("route_id"),
                row.get::<String, _>("runtime_name"),
            )
        })
        .collect();

    Ok(runs)
}
