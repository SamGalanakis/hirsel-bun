//! Live node CRUD operations and tree building

use sqlx::Row;
use std::collections::HashMap;

use super::{DeltaState, DeltaStateError, DeltaStateResult};
use crate::core::db::utc_now;
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Live Node Operations
    // =========================================================================

    /// Get all live nodes as flat list
    pub async fn get_live_nodes(&self) -> DeltaStateResult<Vec<LiveNode>> {
        let pool = self.pool().await?;

        // Load relationships first
        let validates_map = self.load_live_validates(pool).await?;
        let validated_by_map = self.load_live_validated_by(pool).await?;
        let blocked_by_map = self.load_live_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at, completed_at, last_commit_sha, resolves, claimed_by, claimed_at, completed_by, eval_result, eval_feedback, tokens_used
             FROM live_nodes
             WHERE project_id = ? AND route_id = ?
             ORDER BY parent_id NULLS FIRST, position",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                LiveNode {
                    id: id.clone(),
                    project_id: row.get("project_id"),
                    draft_node_id: row.get("draft_node_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    status: LiveNodeStatus::from_str(&row.get::<String, _>("status")),
                    source: LiveNodeSource::from_str(&row.get::<String, _>("source")),
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    validated_by: validated_by_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    completed_at: row.get("completed_at"),
                    last_commit_sha: row.get("last_commit_sha"),
                    resolves: row.get("resolves"),
                    claimed_by: row.get("claimed_by"),
                    claimed_at: row.get("claimed_at"),
                    completed_by: row.get("completed_by"),
                    eval_result: row
                        .get::<Option<String>, _>("eval_result")
                        .and_then(|s| EvalResult::from_str(&s)),
                    eval_feedback: row.get("eval_feedback"),
                    tokens_used: row.get("tokens_used"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Get a single live node
    pub async fn get_live_node(&self, id: &str) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;

        let validates = self.load_live_node_validates(pool, id).await?;
        let validated_by = self.load_live_node_validated_by(pool, id).await?;
        let blocked_by = self.load_live_node_blocked_by(pool, id).await?;

        let row = sqlx::query(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at, completed_at, last_commit_sha, resolves, claimed_by, claimed_at, completed_by, eval_result, eval_feedback, tokens_used
             FROM live_nodes
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::LiveNodeNotFound(id.to_string()))?;

        Ok(LiveNode {
            id: row.get("id"),
            project_id: row.get("project_id"),
            draft_node_id: row.get("draft_node_id"),
            parent_id: row.get("parent_id"),
            position: row.get("position"),
            name: row.get("name"),
            node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
            content: row.get("content"),
            status: LiveNodeStatus::from_str(&row.get::<String, _>("status")),
            source: LiveNodeSource::from_str(&row.get::<String, _>("source")),
            validates,
            validated_by,
            blocked_by,
            x: row.get("x"),
            y: row.get("y"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            completed_at: row.get("completed_at"),
            last_commit_sha: row.get("last_commit_sha"),
            resolves: row.get("resolves"),
            claimed_by: row.get("claimed_by"),
            claimed_at: row.get("claimed_at"),
            completed_by: row.get("completed_by"),
            eval_result: row
                .get::<Option<String>, _>("eval_result")
                .and_then(|s| EvalResult::from_str(&s)),
            eval_feedback: row.get("eval_feedback"),
            tokens_used: row.get("tokens_used"),
        })
    }

    /// Create a live node from a draft node
    pub async fn create_live_node_from_draft(
        &self,
        draft: &DraftNode,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "INSERT INTO live_nodes (id, project_id, route_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', 'spec', ?, ?, ?, ?)",
        )
        .bind(&draft.id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(&draft.id) // draft_node_id = draft.id initially
        .bind(&draft.parent_id)
        .bind(draft.position)
        .bind(&draft.name)
        .bind(draft.node_type.as_str())
        .bind(&draft.content)
        .bind(draft.x)
        .bind(draft.y)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validated_by relationships (task declares which evals validate it)
        for eval_id in &draft.validated_by {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_validated_by (eval_id, task_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(eval_id)
            .bind(&draft.id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships (OR IGNORE handles duplicates)
        for blocker_id in &draft.blocked_by {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(&draft.id)
            .bind(blocker_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        self.get_live_node(&draft.id).await
    }

    /// Create a live node added by a worker (not from draft)
    ///
    /// These nodes have no draft_node_id and source='worker'.
    /// Used when workers call add_task MCP to add tasks during execution.
    pub async fn create_live_node_from_worker(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        node_type: NodeType,
        content: &str,
        validates: Option<&[&str]>,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Get position (append after siblings)
        let position: i32 = match parent_id {
            Some(pid) => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM live_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ? AND route_id = ?",
                )
                .bind(pid)
                .bind(self.project_id)
                .bind(self.route_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
            None => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM live_nodes WHERE parent_id IS NULL AND project_id = ? AND route_id = ?",
                )
                .bind(self.project_id)
                .bind(self.route_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
        };

        sqlx::query(
            "INSERT INTO live_nodes (id, project_id, route_id, draft_node_id, parent_id, position, name, node_type, content, status, source, created_at, updated_at)
             VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?, 'pending', 'worker', ?, ?)",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(parent_id)
        .bind(position)
        .bind(name)
        .bind(node_type.as_str())
        .bind(content)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert blocked_by relationships
        if let Some(blockers) = blocked_by {
            for blocker_id in blockers {
                sqlx::query(
                    "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(blocker_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
            }
        }

        // For eval nodes with validates (convenience sugar): write validated_by on target tasks
        if let Some(task_ids) = validates {
            for task_id in task_ids {
                sqlx::query(
                    "INSERT OR IGNORE INTO live_node_validated_by (eval_id, task_id, project_id, route_id) VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(task_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
            }
        }

        self.bump_tree_generation().await?;
        self.get_live_node(id).await
    }

    /// Add scope task as a blocker to currently claimable nodes
    ///
    /// Called after creating the scope task on first dispatch.
    /// Only blocks nodes that would be claimable right now (no existing blockers,
    /// pending, unclaimed, leaf nodes). This avoids redundant blocking edges.
    pub async fn add_scope_blocking_to_roots(&self) -> DeltaStateResult<()> {
        // Get nodes that are currently claimable (would be workable if scope didn't exist)
        let claimable = self.get_claimable_nodes().await?;

        let pool = self.pool().await?;

        // Add scope as blocker only to claimable nodes (excluding scope itself)
        for node in claimable {
            if node.id != "scope" {
                sqlx::query(
                    "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id, route_id)
                     VALUES (?, 'scope', ?, ?)",
                )
                .bind(&node.id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
            }
        }

        Ok(())
    }

    /// Update live node status
    pub async fn update_live_node_status(
        &self,
        id: &str,
        status: LiveNodeStatus,
        commit_sha: Option<&str>,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Set completed_at on terminal statuses
        let completed_at = if matches!(
            status,
            LiveNodeStatus::Done | LiveNodeStatus::Failed | LiveNodeStatus::Validated
        ) {
            Some(now.clone())
        } else {
            None
        };

        sqlx::query(
            "UPDATE live_nodes SET status = ?, completed_at = ?, last_commit_sha = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(status.as_str())
        .bind(completed_at)
        .bind(commit_sha)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_live_node(id).await
    }

    /// Update live node content (for modify deltas)
    pub async fn update_live_node_from_draft(
        &self,
        draft: &DraftNode,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE live_nodes SET name = ?, content = ?, x = ?, y = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(&draft.name)
        .bind(&draft.content)
        .bind(draft.x)
        .bind(draft.y)
        .bind(&now)
        .bind(&draft.id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // Replace validated_by relationships (task declares which evals validate it)
        sqlx::query(
            "DELETE FROM live_node_validated_by WHERE task_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(&draft.id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;
        for eval_id in &draft.validated_by {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_validated_by (eval_id, task_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(eval_id)
            .bind(&draft.id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        // Replace blocked_by relationships (OR IGNORE handles duplicates in list)
        sqlx::query(
            "DELETE FROM live_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(&draft.id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;
        for blocker_id in &draft.blocked_by {
            sqlx::query(
                "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(&draft.id)
            .bind(blocker_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.get_live_node(&draft.id).await
    }

    /// Delete a live node and associated submissions
    pub async fn delete_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Delete associated delta_submissions first (cascade)
        sqlx::query(
            "DELETE FROM delta_submissions WHERE live_node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // Delete validates relationships (both as eval and as referenced task)
        sqlx::query(
            "DELETE FROM live_node_validated_by WHERE eval_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;
        sqlx::query(
            "DELETE FROM live_node_validated_by WHERE task_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // Delete blocked_by relationships (both as blocker and as blocked)
        sqlx::query(
            "DELETE FROM live_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;
        sqlx::query(
            "DELETE FROM live_node_blocked_by WHERE blocker_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // Delete the live node
        let result =
            sqlx::query("DELETE FROM live_nodes WHERE id = ? AND project_id = ? AND route_id = ?")
                .bind(id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
        if result.rows_affected() == 0 {
            return Err(DeltaStateError::LiveNodeNotFound(id.to_string()));
        }
        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Build tree from flat live nodes
    ///
    /// Uses recursive approach to ensure all descendants are included.
    /// blocked_by is now stored per-node, not computed from validates.
    pub fn build_live_tree(&self, nodes: &[LiveNode]) -> Vec<LiveNodeTree> {
        // Group children by parent
        let mut children_by_parent: HashMap<Option<String>, Vec<&LiveNode>> = HashMap::new();
        for node in nodes {
            children_by_parent
                .entry(node.parent_id.clone())
                .or_default()
                .push(node);
        }

        // Sort children by position
        for children in children_by_parent.values_mut() {
            children.sort_by_key(|n| n.position);
        }

        // Recursive tree builder
        fn build_node(
            node: &LiveNode,
            children_by_parent: &HashMap<Option<String>, Vec<&LiveNode>>,
        ) -> LiveNodeTree {
            let children: Vec<LiveNodeTree> = children_by_parent
                .get(&Some(node.id.clone()))
                .map(|kids| {
                    kids.iter()
                        .map(|child| build_node(child, children_by_parent))
                        .collect()
                })
                .unwrap_or_default();

            LiveNodeTree {
                id: node.id.clone(),
                draft_node_id: node.draft_node_id.clone(),
                parent_id: node.parent_id.clone(),
                name: node.name.clone(),
                node_type: node.node_type,
                content: node.content.clone(),
                status: node.status,
                source: node.source,
                validates: node.validates.clone(),
                validated_by: node.validated_by.clone(),
                blocked_by: node.blocked_by.clone(),
                completed_at: node.completed_at.clone(),
                last_commit_sha: node.last_commit_sha.clone(),
                resolves: node.resolves.clone(),
                children,
                x: node.x,
                y: node.y,
                claimed_by: node.claimed_by.clone(),
                claimed_at: node.claimed_at.clone(),
                completed_by: node.completed_by.clone(),
                eval_result: node.eval_result,
                eval_feedback: node.eval_feedback.clone(),
                tokens_used: node.tokens_used,
            }
        }

        // Build from roots (nodes with no parent)
        children_by_parent
            .get(&None)
            .map(|roots| {
                roots
                    .iter()
                    .map(|root| build_node(root, &children_by_parent))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get live tree for project
    pub async fn get_live_tree(&self) -> DeltaStateResult<Vec<LiveNodeTree>> {
        let nodes = self.get_live_nodes().await?;
        Ok(self.build_live_tree(&nodes))
    }

    /// Get direct children of a live node
    pub async fn get_children(&self, id: &str) -> DeltaStateResult<Vec<LiveNode>> {
        let pool = self.pool().await?;

        // Load relationships
        let validates_map = self.load_live_validates(pool).await?;
        let validated_by_map = self.load_live_validated_by(pool).await?;
        let blocked_by_map = self.load_live_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, draft_node_id, parent_id, position, name, node_type, content, status, source, x, y, created_at, updated_at, completed_at, last_commit_sha, resolves, claimed_by, claimed_at, completed_by, eval_result, eval_feedback, tokens_used
             FROM live_nodes
             WHERE parent_id = ? AND project_id = ? AND route_id = ?
             ORDER BY position",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let node_id: String = row.get("id");
                LiveNode {
                    id: node_id.clone(),
                    project_id: row.get("project_id"),
                    draft_node_id: row.get("draft_node_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    status: LiveNodeStatus::from_str(&row.get::<String, _>("status")),
                    source: LiveNodeSource::from_str(&row.get::<String, _>("source")),
                    validates: validates_map.get(&node_id).cloned().unwrap_or_default(),
                    validated_by: validated_by_map.get(&node_id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&node_id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    completed_at: row.get("completed_at"),
                    last_commit_sha: row.get("last_commit_sha"),
                    resolves: row.get("resolves"),
                    claimed_by: row.get("claimed_by"),
                    claimed_at: row.get("claimed_at"),
                    completed_by: row.get("completed_by"),
                    eval_result: row
                        .get::<Option<String>, _>("eval_result")
                        .and_then(|s| EvalResult::from_str(&s)),
                    eval_feedback: row.get("eval_feedback"),
                    tokens_used: row.get("tokens_used"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Check if a node has children
    pub async fn has_children(&self, id: &str) -> DeltaStateResult<bool> {
        let pool = self.pool().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM live_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;
        Ok(count > 0)
    }

    /// Delete a live node by ID
    ///
    /// This is used for task deletion from workers - removes the node
    /// and cleans up relationships.
    pub async fn delete_live_node_by_id(&self, id: &str) -> DeltaStateResult<()> {
        self.delete_live_node(id).await
    }

    /// Get a lightweight map of worker_name -> task_name for currently working nodes.
    ///
    /// This avoids loading the full live tree with all relationships just to build
    /// the claimed task mapping for the workers display.
    pub async fn get_claimed_task_map(&self) -> DeltaStateResult<HashMap<String, String>> {
        let pool = self.pool().await?;

        let rows = sqlx::query(
            "SELECT claimed_by, name FROM live_nodes
             WHERE project_id = ? AND route_id = ? AND status = 'working' AND claimed_by IS NOT NULL",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let claimed_by: String = row.get("claimed_by");
                let name: String = row.get("name");
                (claimed_by, name)
            })
            .collect())
    }

    /// Get summary counts for live nodes (done, total) without loading full data.
    pub async fn get_live_node_counts(&self) -> DeltaStateResult<(u32, u32)> {
        let pool = self.pool().await?;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM live_nodes WHERE project_id = ? AND route_id = ?",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;

        let done: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM live_nodes WHERE project_id = ? AND route_id = ? AND status IN ('done', 'validated')",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;

        Ok((done as u32, total as u32))
    }
}
