//! Junction table helpers for validated_by and blocked_by relationships

use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

use super::{DeltaState, DeltaStateResult};

impl DeltaState {
    // =========================================================================
    // Draft Node Relations
    // =========================================================================

    /// Load all draft validates relationships (eval -> tasks), computed from validated_by table
    pub(crate) async fn load_draft_validates(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT eval_id, task_id FROM draft_node_validated_by WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let eval_id: String = row.get("eval_id");
            let task_id: String = row.get("task_id");
            map.entry(eval_id).or_default().push(task_id);
        }
        Ok(map)
    }

    /// Load all draft validated_by relationships (task -> evals)
    pub(crate) async fn load_draft_validated_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT eval_id, task_id FROM draft_node_validated_by WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let eval_id: String = row.get("eval_id");
            let task_id: String = row.get("task_id");
            map.entry(task_id).or_default().push(eval_id);
        }
        Ok(map)
    }

    /// Load all draft blocked_by relationships (node -> blockers)
    pub(crate) async fn load_draft_blocked_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT node_id, blocker_id FROM draft_node_blocked_by WHERE project_id = ? AND route_id = ?",
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

    /// Load validates for a single draft eval node (computed from validated_by)
    pub(crate) async fn load_draft_node_validates(
        &self,
        pool: &SqlitePool,
        eval_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT task_id FROM draft_node_validated_by WHERE eval_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(eval_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load validated_by for a single draft task node
    pub(crate) async fn load_draft_node_validated_by(
        &self,
        pool: &SqlitePool,
        task_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT eval_id FROM draft_node_validated_by WHERE task_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(task_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load blocked_by for a single draft node
    pub(crate) async fn load_draft_node_blocked_by(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT blocker_id FROM draft_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    // =========================================================================
    // Live Node Relations
    // =========================================================================

    /// Load all live validates relationships (eval -> tasks), computed from validated_by table
    pub(crate) async fn load_live_validates(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT eval_id, task_id FROM live_node_validated_by WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let eval_id: String = row.get("eval_id");
            let task_id: String = row.get("task_id");
            map.entry(eval_id).or_default().push(task_id);
        }
        Ok(map)
    }

    /// Load all live validated_by relationships (task -> evals)
    pub(crate) async fn load_live_validated_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT eval_id, task_id FROM live_node_validated_by WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let eval_id: String = row.get("eval_id");
            let task_id: String = row.get("task_id");
            map.entry(task_id).or_default().push(eval_id);
        }
        Ok(map)
    }

    /// Load all live blocked_by relationships (node -> blockers)
    pub(crate) async fn load_live_blocked_by(
        &self,
        pool: &SqlitePool,
    ) -> DeltaStateResult<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT node_id, blocker_id FROM live_node_blocked_by WHERE project_id = ? AND route_id = ?",
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

    /// Load validates for a single live eval node (computed from validated_by)
    pub(crate) async fn load_live_node_validates(
        &self,
        pool: &SqlitePool,
        eval_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT task_id FROM live_node_validated_by WHERE eval_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(eval_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load validated_by for a single live task node
    pub(crate) async fn load_live_node_validated_by(
        &self,
        pool: &SqlitePool,
        task_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT eval_id FROM live_node_validated_by WHERE task_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(task_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Load blocked_by for a single live node
    pub(crate) async fn load_live_node_blocked_by(
        &self,
        pool: &SqlitePool,
        node_id: &str,
    ) -> DeltaStateResult<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT blocker_id FROM live_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }
}
