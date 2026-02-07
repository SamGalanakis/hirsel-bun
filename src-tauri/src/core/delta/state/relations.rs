//! Junction table helpers for validated_by and blocked_by relationships

use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

use super::{DeltaState, DeltaStateResult};

impl DeltaState {
    // =========================================================================
    // Board Node Relations (unified)
    // =========================================================================

    /// Load all validates relationships (check -> nodes), computed from checked_by table
    pub(crate) async fn load_validates(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT check_id, node_id FROM board_node_checked_by WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let check_id: String = row.get("check_id");
            let node_id: String = row.get("node_id");
            map.entry(check_id).or_default().push(node_id);
        }
        Ok(map)
    }

    /// Load all validated_by relationships (node -> checks)
    pub(crate) async fn load_validated_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT check_id, node_id FROM board_node_checked_by WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let check_id: String = row.get("check_id");
            let node_id: String = row.get("node_id");
            map.entry(node_id).or_default().push(check_id);
        }
        Ok(map)
    }

    /// Load all blocked_by relationships (node -> blockers)
    pub(crate) async fn load_blocked_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT node_id, blocker_id FROM board_node_blocked_by WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let node_id: String = row.get("node_id");
            let blocker_id: String = row.get("blocker_id");
            map.entry(node_id).or_default().push(blocker_id);
        }
        Ok(map)
    }

    /// Load validates for a single check node
    pub(crate) async fn load_node_validates(
        &self,
        pool: &SqlitePool,
        check_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT node_id FROM board_node_checked_by WHERE check_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(check_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load validated_by for a single node
    pub(crate) async fn load_node_validated_by(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT check_id FROM board_node_checked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Add a blocked_by relationship
    pub async fn add_blocked_by(&self, node_id: &str, blocker_id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id)
             VALUES (?, ?, ?, ?)",
        )
        .bind(node_id)
        .bind(blocker_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Add a validated_by relationship
    pub async fn add_validated_by(&self, node_id: &str, check_id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO board_node_checked_by (node_id, check_id, project_id, route_id)
             VALUES (?, ?, ?, ?)",
        )
        .bind(node_id)
        .bind(check_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Remove a blocked_by relationship
    pub async fn remove_blocked_by(&self, node_id: &str, blocker_id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        sqlx::query(
            "DELETE FROM board_node_blocked_by WHERE node_id = ? AND blocker_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(node_id)
        .bind(blocker_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Load blocked_by for a single node
    pub(crate) async fn load_node_blocked_by(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT blocker_id FROM board_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }
}
