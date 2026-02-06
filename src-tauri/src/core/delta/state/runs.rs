//! Project run operations

use sqlx::Row;

use super::{ensure_schema, DeltaState, DeltaStateResult};
use crate::core::db::{global_pool, utc_now};
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Project Run Operations
    // =========================================================================

    /// Get the persistent run for this project
    pub async fn get_project_run(&self) -> DeltaStateResult<Option<ProjectRun>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, route_id, run_name, status, created_at, last_dispatch_at
             FROM project_runs
             WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| self.row_to_project_run(&row)))
    }

    /// Create a new persistent run for this project
    pub async fn create_project_run(&self, run_name: &str) -> DeltaStateResult<ProjectRun> {
        let pool = self.pool().await?;
        let now = utc_now();

        let result = sqlx::query(
            "INSERT INTO project_runs (project_id, route_id, run_name, status, created_at)
             VALUES (?, ?, ?, 'paused', ?)",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(run_name)
        .bind(&now)
        .execute(pool)
        .await?;

        let id = result.last_insert_rowid();
        self.bump_tree_generation().await?;
        Ok(ProjectRun {
            id,
            project_id: self.project_id,
            route_id: self.route_id,
            run_name: run_name.to_string(),
            status: ProjectRunStatus::Paused,
            created_at: now,
            last_dispatch_at: None,
        })
    }

    /// Update project run status
    pub async fn update_project_run_status(
        &self,
        status: ProjectRunStatus,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query("UPDATE project_runs SET status = ? WHERE project_id = ? AND route_id = ?")
            .bind(status.as_str())
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Record dispatch time
    pub async fn record_dispatch(&self) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE project_runs SET last_dispatch_at = ? WHERE project_id = ? AND route_id = ?",
        )
        .bind(&now)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub(crate) fn row_to_project_run(&self, row: &sqlx::sqlite::SqliteRow) -> ProjectRun {
        let run_name: String = row.get("run_name");
        let status_str: String = row.get("status");
        let status = ProjectRunStatus::from_str(&status_str);

        ProjectRun {
            id: row.get("id"),
            project_id: row.get("project_id"),
            route_id: row.get("route_id"),
            run_name,
            status,
            created_at: row.get("created_at"),
            last_dispatch_at: row.get("last_dispatch_at"),
        }
    }

    /// List all project runs across all projects (for run listing)
    ///
    /// Returns tuples of (ProjectRun, project_name) for building run summaries.
    pub async fn list_all_project_runs() -> DeltaStateResult<Vec<(ProjectRun, String)>> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;

        let rows = sqlx::query(
            "SELECT pr.id, pr.project_id, pr.route_id, pr.run_name, pr.status, pr.created_at, pr.last_dispatch_at,
                    p.name as project_name
             FROM project_runs pr
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
                    ProjectRun {
                        id: row.get("id"),
                        project_id: row.get("project_id"),
                        route_id: row.get("route_id"),
                        run_name: row.get("run_name"),
                        status: ProjectRunStatus::from_str(&status_str),
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
