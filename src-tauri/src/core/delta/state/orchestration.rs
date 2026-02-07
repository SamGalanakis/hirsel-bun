//! Orchestration operations (task claiming, completion, check)

use super::{DeltaState, DeltaStateError, DeltaStateResult};
use crate::core::db::utc_now;
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Orchestration Operations (Task Claiming, Completion, Check)
    // =========================================================================

    /// Claim a board node for a worker
    pub async fn claim_node(&self, id: &str, worker_name: &str) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_node(id).await?;
        if node.kind == NodeKind::Feature {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is a feature and cannot be claimed directly",
                id
            )));
        }
        if node.status != BoardNodeStatus::Pending {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is not in pending status (current: {:?})",
                id, node.status
            )));
        }
        if node.claimed_by.is_some() {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is already claimed by {:?}",
                id, node.claimed_by
            )));
        }

        if self.is_node_blocked(id).await? {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is blocked and cannot be claimed",
                id
            )));
        }

        sqlx::query(
            "UPDATE board_nodes SET status = ?, claimed_by = ?, claimed_at = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(BoardNodeStatus::Working.as_str())
        .bind(worker_name)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Unclaim a board node (worker gives up the task)
    pub async fn unclaim_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE board_nodes SET status = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(BoardNodeStatus::Pending.as_str())
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Complete a board node (mark as done by worker)
    pub async fn complete_node(&self, id: &str, worker_name: &str) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_node(id).await?;

        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is not claimed by {}",
                id, worker_name
            )));
        }

        let new_status = if matches!(node.kind, NodeKind::Task | NodeKind::Feature)
            && self.has_validating_check(id).await?
        {
            BoardNodeStatus::AwaitingCheck
        } else {
            BoardNodeStatus::Done
        };

        sqlx::query(
            "UPDATE board_nodes SET status = ?, completed_at = ?, completed_by = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(new_status.as_str())
        .bind(&now)
        .bind(worker_name)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.propagate_parent_status(id).await?;

        if let Some(eval_id) = &node.resolves {
            self.handle_repair_completion(id, eval_id).await?;
        }

        self.check_project_run_completion().await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Check if all dispatched nodes are complete and pause the project run if so
    pub async fn check_project_run_completion(&self) -> DeltaStateResult<()> {
        let nodes = self.get_nodes().await?;
        let active_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.status != BoardNodeStatus::Draft)
            .collect();

        if active_nodes.is_empty() {
            return Ok(());
        }

        let all_done = active_nodes.iter().all(|n| {
            n.status == BoardNodeStatus::Done
                || n.status == BoardNodeStatus::Failed
                || n.status == BoardNodeStatus::Validated
        });

        if all_done {
            tracing::info!(
                "All board nodes complete for project {}, pausing run",
                self.project_id
            );
            self.update_project_run_status(ProjectRunStatus::Paused)
                .await?;
        }

        Ok(())
    }

    /// Check if a node has a validating check
    pub async fn has_validating_check(&self, node_id: &str) -> DeltaStateResult<bool> {
        let pool = self.pool().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_node_checked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;
        Ok(count > 0)
    }

    /// Get all check IDs that validate a given node
    pub async fn get_checks_for_node(&self, node_id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
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

    /// Propagate status changes up to parent nodes.
    async fn propagate_parent_status(&self, child_id: &str) -> DeltaStateResult<()> {
        let child = self.get_node(child_id).await?;

        let parent_id = match &child.parent_id {
            Some(pid) => pid.clone(),
            None => return Ok(()),
        };

        let siblings = self.get_children(&parent_id).await?;
        if siblings.is_empty() {
            return Ok(());
        }

        let all_validated = siblings
            .iter()
            .all(|s| s.status == BoardNodeStatus::Validated);
        let all_done_or_validated = siblings.iter().all(|s| {
            s.status == BoardNodeStatus::Done
                || s.status == BoardNodeStatus::AwaitingCheck
                || s.status == BoardNodeStatus::Validated
        });

        let new_status = if all_validated {
            if self.has_validating_check(&parent_id).await? {
                let parent = self.get_node(&parent_id).await?;
                if parent.status == BoardNodeStatus::Validated {
                    return Ok(());
                }
                BoardNodeStatus::AwaitingCheck
            } else {
                BoardNodeStatus::Validated
            }
        } else if all_done_or_validated {
            if self.has_validating_check(&parent_id).await? {
                BoardNodeStatus::AwaitingCheck
            } else {
                BoardNodeStatus::Done
            }
        } else {
            return Ok(());
        };

        let pool = self.pool().await?;
        let now = utc_now();
        sqlx::query(
            "UPDATE board_nodes SET status = ?, updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ? AND status NOT IN ('done', 'awaiting_check', 'validated')",
        )
        .bind(new_status.as_str())
        .bind(&now)
        .bind(&parent_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        Box::pin(self.propagate_parent_status(&parent_id)).await
    }

    /// Handle completion of a repair task
    async fn handle_repair_completion(
        &self,
        repair_id: &str,
        eval_id: &str,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "DELETE FROM board_node_blocked_by
             WHERE node_id = ? AND blocker_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(eval_id)
        .bind(repair_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        sqlx::query(
            "UPDATE board_nodes
             SET status = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(BoardNodeStatus::Pending.as_str())
        .bind(&now)
        .bind(eval_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        let checked_node_ids = self.get_checked_nodes(eval_id).await?;
        for node_id in &checked_node_ids {
            sqlx::query(
                "UPDATE board_nodes
                 SET status = ?, updated_at = ?
                 WHERE id = ? AND project_id = ? AND route_id = ? AND status = ?",
            )
            .bind(BoardNodeStatus::AwaitingCheck.as_str())
            .bind(&now)
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .bind(BoardNodeStatus::NeedsRepair.as_str())
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Get all node IDs validated by a check
    pub async fn get_checked_nodes(&self, check_id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
        let node_ids: Vec<String> = sqlx::query_scalar(
            "SELECT node_id FROM board_node_checked_by WHERE check_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(check_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(node_ids)
    }

    /// Handle check pass
    pub async fn check_pass(&self, check_id: &str, worker_name: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_node(check_id).await?;

        if node.kind != NodeKind::Check {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is not a check node",
                check_id
            )));
        }

        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Check '{}' is not claimed by {}",
                check_id, worker_name
            )));
        }

        let checked_node_ids = self.get_checked_nodes(check_id).await?;

        sqlx::query(
            "UPDATE board_nodes SET status = ?, completed_at = ?, completed_by = ?, check_result = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(BoardNodeStatus::Done.as_str())
        .bind(&now)
        .bind(worker_name)
        .bind(CheckResult::Pass.as_str())
        .bind(&now)
        .bind(check_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        for node_id in &checked_node_ids {
            let check_ids = self.get_checks_for_node(node_id).await?;
            let all_passed = if check_ids.is_empty() {
                true
            } else {
                let mut all = true;
                for cid in &check_ids {
                    let check_node = match self.get_node(cid).await {
                        Ok(n) => n,
                        Err(_) => {
                            all = false;
                            break;
                        }
                    };
                    if check_node.check_result != Some(CheckResult::Pass) {
                        all = false;
                        break;
                    }
                }
                all
            };

            if all_passed {
                sqlx::query(
                    "UPDATE board_nodes SET status = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ? AND status IN (?, ?)",
                )
                .bind(BoardNodeStatus::Validated.as_str())
                .bind(&now)
                .bind(node_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .bind(BoardNodeStatus::Done.as_str())
                .bind(BoardNodeStatus::AwaitingCheck.as_str())
                .execute(pool)
                .await?;
            }
        }

        for node_id in &checked_node_ids {
            self.propagate_parent_status(node_id).await?;
        }

        self.propagate_parent_status(check_id).await?;
        self.check_project_run_completion().await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Handle check fail - creates a repair node
    pub async fn check_fail(
        &self,
        check_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> DeltaStateResult<String> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_node(check_id).await?;

        if node.kind != NodeKind::Check {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is not a check node",
                check_id
            )));
        }

        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Check '{}' is not claimed by {}",
                check_id, worker_name
            )));
        }

        let checked_node_ids = self.get_checked_nodes(check_id).await?;

        let repair_id = format!("{}-repair-{}", check_id, now.replace([':', '-', '.'], ""));
        let repair_name = format!("Repair: {}", feedback.chars().take(50).collect::<String>());

        sqlx::query(
            "INSERT INTO board_nodes (id, project_id, route_id, parent_id, position, name, kind, source, content, status, resolves, created_at, updated_at)
             VALUES (?, ?, ?, NULL, 0, ?, 'task', 'system', ?, 'pending', ?, ?, ?)",
        )
        .bind(&repair_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(&repair_name)
        .bind(feedback)
        .bind(check_id)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        sqlx::query(
            "UPDATE board_nodes SET status = ?, check_result = ?, check_feedback = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(BoardNodeStatus::Pending.as_str())
        .bind(CheckResult::Fail.as_str())
        .bind(feedback)
        .bind(&now)
        .bind(check_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        sqlx::query(
            "INSERT OR IGNORE INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
        )
        .bind(check_id)
        .bind(&repair_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        for node_id in &checked_node_ids {
            sqlx::query(
                "UPDATE board_nodes SET status = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ? AND status IN (?, ?)",
            )
            .bind(BoardNodeStatus::NeedsRepair.as_str())
            .bind(&now)
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .bind(BoardNodeStatus::Done.as_str())
            .bind(BoardNodeStatus::AwaitingCheck.as_str())
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        Ok(repair_id)
    }

    /// Check if a node is blocked — propagates up the parent chain,
    /// so a node is blocked if it or any ancestor is blocked.
    pub fn is_node_blocked(
        &self,
        node_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = DeltaStateResult<bool>> + Send + '_>>
    {
        let node_id = node_id.to_string();
        Box::pin(async move {
            let node = self.get_node(&node_id).await?;

            // Propagate up: if any ancestor is blocked, so are we
            if let Some(parent_id) = &node.parent_id {
                if self.is_node_blocked(parent_id).await? {
                    return Ok(true);
                }
            }

            match node.kind {
                NodeKind::Task | NodeKind::Feature => {
                    if node.blocked_by.is_empty() {
                        return Ok(false);
                    }

                    for blocker_id in &node.blocked_by {
                        let blocker = match self.get_node(blocker_id).await {
                            Ok(b) => b,
                            Err(_) => {
                                tracing::debug!(
                                    "is_node_blocked({}): blocker '{}' not found, treating as blocked",
                                    node_id,
                                    blocker_id
                                );
                                return Ok(true);
                            }
                        };

                        let has_check = self.has_validating_check(blocker_id).await?;
                        let is_blocking = if has_check {
                            blocker.status != BoardNodeStatus::Validated
                        } else {
                            !blocker.status.is_complete()
                        };

                        if is_blocking {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                NodeKind::Check => {
                    let validates = self.get_checked_nodes(&node_id).await?;

                    if validates.is_empty() {
                        // No explicit validates targets — infer scope from position.
                        // A check under a parent waits for all non-check siblings.
                        // A root-level check waits for all non-check nodes in the project.
                        let scope_nodes = if let Some(ref pid) = node.parent_id {
                            self.get_children(pid).await?
                        } else {
                            self.get_nodes().await?
                        };

                        for sibling in &scope_nodes {
                            if sibling.id == node_id || sibling.kind == NodeKind::Check {
                                continue;
                            }
                            if sibling.status == BoardNodeStatus::Draft {
                                continue;
                            }
                            let is_work_done = matches!(
                                sibling.status,
                                BoardNodeStatus::Done
                                    | BoardNodeStatus::AwaitingCheck
                                    | BoardNodeStatus::Validated
                            );
                            if !is_work_done {
                                return Ok(true);
                            }
                        }
                    } else {
                        for target_id in &validates {
                            let target = match self.get_node(target_id).await {
                                Ok(t) => t,
                                Err(_) => return Ok(true),
                            };

                            let is_ready = matches!(
                                target.status,
                                BoardNodeStatus::Done
                                    | BoardNodeStatus::AwaitingCheck
                                    | BoardNodeStatus::Validated
                            );
                            if !is_ready {
                                return Ok(true);
                            }
                        }
                    }

                    for blocker_id in &node.blocked_by {
                        let blocker = match self.get_node(blocker_id).await {
                            Ok(b) => b,
                            Err(_) => return Ok(true),
                        };
                        if !blocker.status.is_complete() {
                            return Ok(true);
                        }
                    }

                    Ok(false)
                }
            }
        })
    }

    /// Get nodes that can be claimed (unblocked, unclaimed, pending status, no children)
    pub async fn get_claimable_nodes(&self) -> DeltaStateResult<Vec<BoardNode>> {
        let nodes = self.get_nodes().await?;

        let nodes_with_children: std::collections::HashSet<String> =
            nodes.iter().filter_map(|n| n.parent_id.clone()).collect();

        let mut eval_nodes = vec![];
        let mut work_nodes = vec![];

        for node in nodes {
            if node.status != BoardNodeStatus::Pending || node.claimed_by.is_some() {
                continue;
            }

            if nodes_with_children.contains(&node.id) {
                continue;
            }

            if self.is_node_blocked(&node.id).await? {
                continue;
            }

            match node.kind {
                NodeKind::Check => eval_nodes.push(node),
                NodeKind::Task => work_nodes.push(node),
                NodeKind::Feature => {} // Features are not directly claimable
            }
        }

        eval_nodes.extend(work_nodes);
        Ok(eval_nodes)
    }

    /// Set tokens used on a node
    pub async fn set_node_tokens(&self, id: &str, tokens: i64) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query("UPDATE board_nodes SET tokens_used = ? WHERE id = ? AND project_id = ? AND route_id = ?")
            .bind(tokens)
            .bind(id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Get the node currently claimed by a worker
    pub async fn get_claimed_node_for_worker(
        &self,
        worker_name: &str,
    ) -> DeltaStateResult<Option<BoardNode>> {
        let pool = self.pool().await?;

        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM board_nodes WHERE claimed_by = ? AND project_id = ? AND route_id = ? AND status = 'working'",
        )
        .bind(worker_name)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;

        match id {
            Some(id) => Ok(Some(self.get_node(&id).await?)),
            None => Ok(None),
        }
    }

    /// Reopen a completed or failed node (reset to pending)
    pub async fn reopen_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE board_nodes SET status = 'pending', claimed_by = NULL, claimed_at = NULL, completed_at = NULL, completed_by = NULL, check_result = NULL, check_feedback = NULL, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.propagate_parent_status(id).await?;
        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Get blocker node IDs for a node
    pub async fn get_blockers(&self, id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
        self.load_node_blocked_by(pool, id).await
    }
}
