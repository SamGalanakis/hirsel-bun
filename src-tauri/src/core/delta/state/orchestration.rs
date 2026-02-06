//! Orchestration operations (task claiming, completion, eval)

use super::{DeltaState, DeltaStateError, DeltaStateResult};
use crate::core::db::utc_now;
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Orchestration Operations (Task Claiming, Completion, Eval)
    // =========================================================================

    /// Claim a live node for a worker
    pub async fn claim_live_node(&self, id: &str, worker_name: &str) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        // Verify node exists and is claimable
        let node = self.get_live_node(id).await?;
        if node.status != LiveNodeStatus::Pending {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not in pending status (current: {:?})",
                id, node.status
            )));
        }
        if node.claimed_by.is_some() {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is already claimed by {:?}",
                id, node.claimed_by
            )));
        }

        // Defense-in-depth: verify node is not blocked before claiming
        if self.is_node_blocked(id).await? {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is blocked and cannot be claimed",
                id
            )));
        }

        sqlx::query(
            "UPDATE live_nodes SET status = ?, claimed_by = ?, claimed_at = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(LiveNodeStatus::Working.as_str())
        .bind(worker_name)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_live_node(id).await
    }

    /// Unclaim a live node (worker gives up the task)
    pub async fn unclaim_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE live_nodes SET status = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(LiveNodeStatus::Pending.as_str())
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Complete a live node (mark as done by worker)
    ///
    /// For work tasks: sets status to Done (or AwaitingEval if it has a validating eval)
    /// For eval tasks: use eval_pass or eval_fail instead
    pub async fn complete_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> DeltaStateResult<LiveNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_live_node(id).await?;

        // Verify claimed by this worker
        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not claimed by {}",
                id, worker_name
            )));
        }

        // Determine new status based on whether there's a validating eval
        let new_status = if node.node_type == NodeType::Task && self.has_validating_eval(id).await?
        {
            LiveNodeStatus::AwaitingEval
        } else {
            LiveNodeStatus::Done
        };

        sqlx::query(
            "UPDATE live_nodes SET status = ?, completed_at = ?, completed_by = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
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

        // Propagate status up to parent
        self.propagate_parent_status(id).await?;

        // Check if this is a repair task completing (resolves an eval)
        if let Some(eval_id) = &node.resolves {
            self.handle_repair_completion(id, eval_id).await?;
        }

        // Check if all live nodes are complete -> pause project run
        self.check_project_run_completion().await?;

        self.bump_tree_generation().await?;
        self.get_live_node(id).await
    }

    /// Check if all live nodes are complete and pause the project run if so
    async fn check_project_run_completion(&self) -> DeltaStateResult<()> {
        let nodes = self.get_live_nodes().await?;
        if nodes.is_empty() {
            return Ok(());
        }

        let all_done = nodes.iter().all(|n| {
            n.status == LiveNodeStatus::Done
                || n.status == LiveNodeStatus::Failed
                || n.status == LiveNodeStatus::Validated
        });

        if all_done {
            tracing::info!(
                "All live nodes complete for project {}, pausing run",
                self.project_id
            );
            self.update_project_run_status(ProjectRunStatus::Paused)
                .await?;
        }

        Ok(())
    }

    /// Check if a task has a validating eval (any eval in validated_by)
    pub async fn has_validating_eval(&self, node_id: &str) -> DeltaStateResult<bool> {
        let pool = self.pool().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM live_node_validated_by WHERE task_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(node_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;
        Ok(count > 0)
    }

    /// Get all eval IDs that validate a given task
    pub async fn get_evals_for_task(&self, task_id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
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

    /// Propagate status changes up to parent nodes.
    ///
    /// Called after a child's status changes. Checks if all siblings have the same
    /// "complete" status and updates parent accordingly. Recursively propagates up.
    async fn propagate_parent_status(&self, child_id: &str) -> DeltaStateResult<()> {
        let child = self.get_live_node(child_id).await?;

        // No parent = nothing to propagate
        let parent_id = match &child.parent_id {
            Some(pid) => pid.clone(),
            None => return Ok(()),
        };

        // Get all siblings (children of same parent)
        let siblings = self.get_children(&parent_id).await?;
        if siblings.is_empty() {
            return Ok(()); // Shouldn't happen, but safety check
        }

        // Check if ALL siblings have same complete status
        let all_validated = siblings
            .iter()
            .all(|s| s.status == LiveNodeStatus::Validated);
        let all_done_or_validated = siblings.iter().all(|s| {
            s.status == LiveNodeStatus::Done
                || s.status == LiveNodeStatus::AwaitingEval
                || s.status == LiveNodeStatus::Validated
        });

        // Determine new parent status
        let new_status = if all_validated {
            // All children validated - parent can be validated
            // BUT: if parent itself has a validating eval, that eval must also pass
            if self.has_validating_eval(&parent_id).await? {
                // Parent needs its own eval to pass - check if already validated
                let parent = self.get_live_node(&parent_id).await?;
                if parent.status == LiveNodeStatus::Validated {
                    // Already validated, nothing to do
                    return Ok(());
                }
                // Set to AwaitingEval - parent's eval can now run
                LiveNodeStatus::AwaitingEval
            } else {
                LiveNodeStatus::Validated
            }
        } else if all_done_or_validated {
            // All children at least "done" (work complete, maybe awaiting/validated)
            if self.has_validating_eval(&parent_id).await? {
                LiveNodeStatus::AwaitingEval
            } else {
                LiveNodeStatus::Done
            }
        } else {
            // Not all children complete - parent stays as-is
            return Ok(());
        };

        // Update parent status (only if not already at or beyond target status)
        let pool = self.pool().await?;
        let now = utc_now();
        sqlx::query(
            "UPDATE live_nodes SET status = ?, updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ? AND status NOT IN ('done', 'awaiting_eval', 'validated')",
        )
        .bind(new_status.as_str())
        .bind(&now)
        .bind(&parent_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // Recursively propagate to grandparent
        Box::pin(self.propagate_parent_status(&parent_id)).await
    }

    /// Handle completion of a repair task
    ///
    /// When a repair task (source=System, child of eval) completes:
    /// 1. Remove blocking relationship so eval is unblocked
    /// 2. Reset eval to pending for re-claiming
    /// 3. Transition needs_repair tasks back to awaiting_eval
    async fn handle_repair_completion(
        &self,
        repair_id: &str,
        eval_id: &str,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        // 1. Remove blocking relationship (eval no longer blocked by repair)
        sqlx::query(
            "DELETE FROM live_node_blocked_by
             WHERE node_id = ? AND blocker_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(eval_id)
        .bind(repair_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // 2. Reset eval to pending so it can be re-claimed
        // Keep eval_result and eval_feedback for history
        sqlx::query(
            "UPDATE live_nodes
             SET status = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(LiveNodeStatus::Pending.as_str())
        .bind(&now)
        .bind(eval_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // 3. Transition needs_repair tasks back to awaiting_eval
        let validated_node_ids = self.get_validated_nodes(eval_id).await?;
        for node_id in &validated_node_ids {
            sqlx::query(
                "UPDATE live_nodes
                 SET status = ?, updated_at = ?
                 WHERE id = ? AND project_id = ? AND route_id = ? AND status = ?",
            )
            .bind(LiveNodeStatus::AwaitingEval.as_str())
            .bind(&now)
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .bind(LiveNodeStatus::NeedsRepair.as_str())
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Get all node IDs validated by an eval
    pub async fn get_validated_nodes(&self, eval_id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
        let node_ids: Vec<String> = sqlx::query_scalar(
            "SELECT task_id FROM live_node_validated_by WHERE eval_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(eval_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;
        Ok(node_ids)
    }

    /// Handle eval pass - validates all nodes in the validates list
    pub async fn eval_pass(&self, eval_id: &str, worker_name: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_live_node(eval_id).await?;

        // Verify it's an eval node
        if node.node_type != NodeType::Eval {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not an eval node",
                eval_id
            )));
        }

        // Verify claimed by this worker
        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Eval '{}' is not claimed by {}",
                eval_id, worker_name
            )));
        }

        // Get validated nodes before updating eval status
        let validated_node_ids = self.get_validated_nodes(eval_id).await?;

        // Mark eval as done with pass result
        sqlx::query(
            "UPDATE live_nodes SET status = ?, completed_at = ?, completed_by = ?, eval_result = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(LiveNodeStatus::Done.as_str())
        .bind(&now)
        .bind(worker_name)
        .bind(EvalResult::Pass.as_str())
        .bind(&now)
        .bind(eval_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // For each validated task, check if ALL its evals have passed before marking Validated
        for node_id in &validated_node_ids {
            let eval_ids = self.get_evals_for_task(node_id).await?;
            let all_passed = if eval_ids.is_empty() {
                true
            } else {
                let mut all = true;
                for eid in &eval_ids {
                    let eval_node = match self.get_live_node(eid).await {
                        Ok(n) => n,
                        Err(_) => {
                            all = false;
                            break;
                        }
                    };
                    if eval_node.eval_result != Some(EvalResult::Pass) {
                        all = false;
                        break;
                    }
                }
                all
            };

            if all_passed {
                sqlx::query(
                    "UPDATE live_nodes SET status = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ? AND status IN (?, ?)",
                )
                .bind(LiveNodeStatus::Validated.as_str())
                .bind(&now)
                .bind(node_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .bind(LiveNodeStatus::Done.as_str())
                .bind(LiveNodeStatus::AwaitingEval.as_str())
                .execute(pool)
                .await?;
            }
        }

        // Propagate status up for each validated task
        for node_id in &validated_node_ids {
            self.propagate_parent_status(node_id).await?;
        }

        // Also propagate for the eval itself (in case eval is child of something)
        self.propagate_parent_status(eval_id).await?;

        // Check if all live nodes are complete -> pause project run
        self.check_project_run_completion().await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Handle eval fail - creates a repair node as child of the eval
    ///
    /// Returns the repair node ID
    pub async fn eval_fail(
        &self,
        eval_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> DeltaStateResult<String> {
        let pool = self.pool().await?;
        let now = utc_now();

        let node = self.get_live_node(eval_id).await?;

        // Verify it's an eval node
        if node.node_type != NodeType::Eval {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Node '{}' is not an eval node",
                eval_id
            )));
        }

        // Verify claimed by this worker
        if node.claimed_by.as_deref() != Some(worker_name) {
            return Err(DeltaStateError::LiveNodeNotFound(format!(
                "Eval '{}' is not claimed by {}",
                eval_id, worker_name
            )));
        }

        // Get validated nodes before creating repair
        let validated_node_ids = self.get_validated_nodes(eval_id).await?;

        // Create repair node that resolves the eval (no parent_id)
        let repair_id = format!("{}-repair-{}", eval_id, now.replace([':', '-', '.'], ""));
        let repair_name = format!("Repair: {}", feedback.chars().take(50).collect::<String>());

        sqlx::query(
            "INSERT INTO live_nodes (id, project_id, route_id, parent_id, position, name, node_type, content, status, source, resolves, created_at, updated_at)
             VALUES (?, ?, ?, NULL, 0, ?, 'task', ?, 'pending', 'system', ?, ?, ?)",
        )
        .bind(&repair_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(&repair_name)
        .bind(feedback)
        .bind(eval_id)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Mark eval as pending (blocked by repair), set feedback
        // Reset claimed_by so it can be reclaimed after repair
        sqlx::query(
            "UPDATE live_nodes SET status = ?, eval_result = ?, eval_feedback = ?, claimed_by = NULL, claimed_at = NULL, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(LiveNodeStatus::Pending.as_str())
        .bind(EvalResult::Fail.as_str())
        .bind(feedback)
        .bind(&now)
        .bind(eval_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // Add blocking relationship (eval is now blocked by repair node)
        sqlx::query(
            "INSERT OR IGNORE INTO live_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
        )
        .bind(eval_id)
        .bind(&repair_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        // Mark validated nodes as needs_repair
        for node_id in &validated_node_ids {
            sqlx::query(
                "UPDATE live_nodes SET status = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ? AND status IN (?, ?)",
            )
            .bind(LiveNodeStatus::NeedsRepair.as_str())
            .bind(&now)
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .bind(LiveNodeStatus::Done.as_str())
            .bind(LiveNodeStatus::AwaitingEval.as_str())
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        Ok(repair_id)
    }

    /// Check if a node is blocked
    ///
    /// For task nodes:
    /// - If blocker has validated_by entries: require Validated
    /// - If blocker has no validated_by: require Done|Validated
    ///
    /// For eval nodes:
    /// - Get tasks where validated_by includes this eval
    /// - Non-empty (targeted eval): all must be Done|AwaitingEval|Validated
    /// - Empty (global eval): check blocked_by only. If also empty → not blocked
    pub async fn is_node_blocked(&self, node_id: &str) -> DeltaStateResult<bool> {
        let node = self.get_live_node(node_id).await?;

        match node.node_type {
            NodeType::Task => {
                if node.blocked_by.is_empty() {
                    return Ok(false);
                }

                for blocker_id in &node.blocked_by {
                    let blocker = match self.get_live_node(blocker_id).await {
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

                    let has_eval = self.has_validating_eval(blocker_id).await?;
                    let is_blocking = if has_eval {
                        // Blocker has validated_by entries → must be Validated
                        blocker.status != LiveNodeStatus::Validated
                    } else {
                        // Blocker has no validated_by → must be Done or Validated
                        !blocker.status.is_complete()
                    };

                    tracing::debug!(
                        "is_node_blocked({}): blocker='{}' status={:?} has_eval={} is_blocking={}",
                        node_id,
                        blocker_id,
                        blocker.status,
                        has_eval,
                        is_blocking
                    );

                    if is_blocking {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeType::Eval => {
                // Get tasks where validated_by includes this eval (targeted eval)
                let validates = self.get_validated_nodes(node_id).await?;

                if validates.is_empty() {
                    // Global eval: check blocked_by only. If also empty → not blocked (immediately claimable)
                    for blocker_id in &node.blocked_by {
                        let blocker = match self.get_live_node(blocker_id).await {
                            Ok(b) => b,
                            Err(_) => return Ok(true),
                        };
                        if !blocker.status.is_complete() {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }

                // Targeted eval: all validated tasks must be Done|AwaitingEval|Validated
                for task_id in &validates {
                    let task = match self.get_live_node(task_id).await {
                        Ok(t) => t,
                        Err(_) => return Ok(true),
                    };

                    let is_ready = matches!(
                        task.status,
                        LiveNodeStatus::Done
                            | LiveNodeStatus::AwaitingEval
                            | LiveNodeStatus::Validated
                    );
                    if !is_ready {
                        return Ok(true);
                    }
                }

                // Also check blocked_by (for repair flow)
                for blocker_id in &node.blocked_by {
                    let blocker = match self.get_live_node(blocker_id).await {
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
    }

    /// Get nodes that can be claimed (unblocked, unclaimed, pending status, no children)
    ///
    /// Priority order:
    /// 1. Eval nodes whose validated nodes are all done/awaiting_eval
    /// 2. Work nodes that are unblocked
    pub async fn get_claimable_nodes(&self) -> DeltaStateResult<Vec<LiveNode>> {
        let nodes = self.get_live_nodes().await?;

        // Build set of nodes that have children
        let nodes_with_children: std::collections::HashSet<String> =
            nodes.iter().filter_map(|n| n.parent_id.clone()).collect();

        let mut eval_nodes = vec![];
        let mut work_nodes = vec![];

        for node in nodes {
            // Must be pending and unclaimed
            if node.status != LiveNodeStatus::Pending || node.claimed_by.is_some() {
                continue;
            }

            // Skip nodes that have children - work on leaf nodes instead
            if nodes_with_children.contains(&node.id) {
                continue;
            }

            // Check if blocked
            if self.is_node_blocked(&node.id).await? {
                continue;
            }

            match node.node_type {
                NodeType::Eval => eval_nodes.push(node),
                NodeType::Task => work_nodes.push(node),
            }
        }

        // Return eval nodes first (higher priority), then work nodes
        eval_nodes.extend(work_nodes);
        Ok(eval_nodes)
    }

    /// Set tokens used on a node
    pub async fn set_node_tokens(&self, id: &str, tokens: i64) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query("UPDATE live_nodes SET tokens_used = ? WHERE id = ? AND project_id = ? AND route_id = ?")
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
    ) -> DeltaStateResult<Option<LiveNode>> {
        let pool = self.pool().await?;

        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM live_nodes WHERE claimed_by = ? AND project_id = ? AND route_id = ? AND status = 'working'",
        )
        .bind(worker_name)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;

        match id {
            Some(id) => Ok(Some(self.get_live_node(&id).await?)),
            None => Ok(None),
        }
    }

    /// Reopen a completed or failed live node (reset to pending)
    pub async fn reopen_live_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE live_nodes SET status = 'pending', claimed_by = NULL, claimed_at = NULL, completed_at = NULL, completed_by = NULL, eval_result = NULL, eval_feedback = NULL, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Get blocker node IDs for a node
    pub async fn get_blockers(&self, id: &str) -> DeltaStateResult<Vec<String>> {
        let pool = self.pool().await?;
        self.load_live_node_blocked_by(pool, id).await
    }
}
