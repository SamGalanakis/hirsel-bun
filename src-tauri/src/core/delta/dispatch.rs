//! Delta dispatch service
//!
//! Orchestrates the delta dispatch flow:
//! 1. Compute diff between draft and live trees
//! 2. Generate delta tasks (implement, modify, revert)
//! 3. Get or create persistent run
//! 4. Add delta tasks to run
//! 5. Update live tree
//! 6. Resume run

use tracing::info;

use super::generator::{DeltaGenerator, GeneratorError};
use super::state::{DeltaState, DeltaStateError};
use super::types::*;
use crate::core::names::generate_run_name;

/// Generate the content for the scope task.
///
/// This content is stored in the scope live node and accessed via MCP get_task_details.
fn generate_scope_content() -> String {
    let mut content = String::new();

    content.push_str("# Scope Task\n\n");
    content.push_str("You are the **leader** for this run. Your job is to review the task tree, understand the work, and unblock other tasks.\n\n");

    content.push_str("## Your Approach\n\n");
    content.push_str("Review the task tree with `get_task_tree()` and decide:\n\n");

    content.push_str("**1. Explore first** - If unfamiliar with codebase:\n");
    content.push_str("   - Create exploration tasks to understand the code\n");
    content.push_str("   - Use `scribe()` to record findings\n");
    content.push_str("   - Create implementation tasks after exploration\n\n");

    content.push_str("**2. Plan more** - If tasks need breakdown:\n");
    content.push_str("   - Create subtasks for large tasks\n");
    content.push_str("   - Add blocking relationships where needed\n\n");

    content.push_str("**3. Start directly** - If tasks are well-defined:\n");
    content.push_str("   - Complete this scope task to unblock other tasks\n");
    content.push_str("   - Begin working on available tasks\n\n");

    content.push_str("When you complete this task, blocked tasks become available.\n\n");

    content.push_str("## Task Design Principles\n\n");
    content.push_str("**Parallel execution:**\n");
    content.push_str("- Minimize dependencies between tasks\n");
    content.push_str("- Prefer vertical slices (complete features) over horizontal layers\n");
    content.push_str("- Tasks touching same files = conflicts. Structure to minimize overlap.\n\n");

    content.push_str("**Dependencies (blocked_by):**\n");
    content.push_str("When in doubt, add the dependency. Better slow than broken:\n");
    content.push_str("- Task reads files another writes? → Add dependency\n");
    content.push_str("- Task calls functions another creates? → Add dependency\n");
    content.push_str("- Task tests code another implements? → Add dependency\n\n");

    content.push_str("## Completing This Task\n\n");
    content.push_str("When you've:\n");
    content.push_str("1. Reviewed the existing task tree\n");
    content.push_str("2. Created any needed exploration/planning tasks\n");
    content.push_str("3. Set up proper blocking relationships\n\n");
    content
        .push_str("Call `work_done()` to complete the scope task and unblock dependent tasks.\n");

    content
}

/// Error type for dispatch operations
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("State error: {0}")]
    State(#[from] DeltaStateError),
    #[error("Generator error: {0}")]
    Generator(#[from] GeneratorError),
    #[error("No changes to dispatch")]
    NoChanges,
    #[error("Run creation failed: {0}")]
    RunCreation(String),
}

pub type DeltaDispatchResult<T> = Result<T, DispatchError>;

/// Service for dispatching delta tasks to runs
pub struct DeltaDispatchService {
    #[allow(dead_code)]
    project_id: i64,
    #[allow(dead_code)]
    route_id: i64,
    state: DeltaState,
    generator: DeltaGenerator,
}

impl DeltaDispatchService {
    /// Create a new dispatch service for a project route
    pub fn new(project_id: i64, route_id: i64) -> Self {
        Self {
            project_id,
            route_id,
            state: DeltaState::with_route(project_id, route_id),
            generator: DeltaGenerator::new(project_id, route_id),
        }
    }

    /// Execute a full dispatch
    ///
    /// 1. Compute diff between draft and live
    /// 2. Generate delta tasks
    /// 3. Get or create persistent run
    /// 4. Create delta submissions in DB
    /// 5. Create board version
    /// 6. Update live tree to reflect new/modified nodes
    /// 7. Return dispatch result
    pub async fn dispatch(&self) -> DeltaDispatchResult<DispatchResult> {
        // 1. Get the diff
        let diff = self.generator.get_diff().await?;

        if diff.is_empty() {
            return Err(DispatchError::NoChanges);
        }

        info!(
            "Dispatching delta: {} new, {} modified, {} deleted",
            diff.new_nodes.len(),
            diff.modified_nodes.len(),
            diff.deleted_nodes.len()
        );

        // 2. Generate delta tasks
        let tasks = self.generator.generate_tasks().await?;

        // 3. Get or create persistent run
        let (run, is_first_dispatch) = self.get_or_create_run().await?;

        // 4. Create delta submissions
        let batch_id = self.state.next_batch_id().await?;
        let submissions = self
            .state
            .create_delta_submissions(&tasks, batch_id)
            .await?;

        info!(
            "Created {} delta submissions in batch {}",
            submissions.len(),
            batch_id
        );

        // 5. Create board version for this dispatch
        let description = Some(diff.summary());
        let version = self
            .state
            .create_board_version(batch_id, description.as_deref())
            .await?;

        info!(
            "Created board version v{} (batch {})",
            version.version_number, batch_id
        );

        // 6. Update live tree
        self.sync_live_tree(&diff, is_first_dispatch).await?;

        // 7. Record dispatch time
        self.state.record_dispatch().await?;

        // 8. Update run status to working
        self.state
            .update_project_run_status(ProjectRunStatus::Working)
            .await?;

        Ok(DispatchResult {
            run_name: run.run_name,
            batch_id,
            delta_count: submissions.len(),
            diff_summary: diff.summary(),
            version_number: version.version_number,
            version_id: version.id,
        })
    }

    /// Get the existing run or create a new one
    /// Returns (run, is_first_dispatch)
    async fn get_or_create_run(&self) -> DeltaDispatchResult<(ProjectRun, bool)> {
        if let Some(run) = self.state.get_project_run().await? {
            info!("Using existing run: {}", run.run_name);
            return Ok((run, false));
        }

        // Create new run
        let run_name = generate_run_name();
        info!("Creating new persistent run: {}", run_name);

        let run = self.state.create_project_run(&run_name).await?;
        Ok((run, true))
    }

    /// Sync live tree to reflect dispatch
    ///
    /// - On first dispatch: create scope node and add it as blocker to root-level tasks
    /// - Create live nodes for new draft nodes (excluding project nodes - UI-only)
    /// - Update live nodes for modified draft nodes
    /// - Mark deleted nodes in live (don't delete yet - revert task will handle)
    async fn sync_live_tree(
        &self,
        diff: &TreeDiff,
        is_first_dispatch: bool,
    ) -> DeltaDispatchResult<()> {
        // On first dispatch, create scope node first with its content
        if is_first_dispatch {
            let scope_content = generate_scope_content();
            self.state
                .create_live_node_from_worker(
                    "scope",
                    "Scope",
                    None,
                    None,
                    NodeType::Task,
                    &scope_content,
                    None, // validates - not used for scope
                )
                .await?;
            info!("Created scope live node with content");
        }

        // Create live nodes for new draft nodes
        for new_node in &diff.new_nodes {
            // Get the full draft node
            if let Ok(draft) = self.state.get_draft_node(&new_node.id).await {
                self.state.create_live_node_from_draft(&draft).await?;
                info!("Created live node: {}", new_node.id);
            }
        }

        // Update live nodes for modified draft nodes
        for modified in &diff.modified_nodes {
            if let Ok(draft) = self.state.get_draft_node(&modified.draft_node.id).await {
                // Check if node is being worked on before updating
                let was_claimed_by = self
                    .state
                    .get_live_node(&draft.id)
                    .await
                    .ok()
                    .and_then(|n| n.claimed_by.clone());

                // Update content from draft
                self.state.update_live_node_from_draft(&draft).await?;

                // Reopen the node (clears claimed_by, completed_at, etc.) and reset to pending
                self.state.reopen_live_node(&draft.id).await?;
                info!("Updated and reopened live node: {}", draft.id);

                // Notify worker if they were working on this task
                if let Some(worker_name) = was_claimed_by {
                    let changes_summary = modified.changes.join("\n- ");
                    let message = format!(
                        "⚠️ Task spec changed by user\n\n\
                         The task you're working on was modified:\n\
                         - {}\n\n\
                         Please adjust your implementation to account for these changes.",
                        changes_summary
                    );

                    // Send message to worker's DM thread
                    if let Ok(store) = crate::core::ProjectMessagesStore::open().await {
                        if let Err(e) = store
                            .add_message(
                                self.project_id,
                                self.route_id,
                                &worker_name,
                                "system",
                                &message,
                                false,
                            )
                            .await
                        {
                            info!(
                                "Failed to send spec change notification to {}: {}",
                                worker_name, e
                            );
                        } else {
                            info!(
                                "Notified worker {} of spec change for node {}",
                                worker_name, draft.id
                            );
                        }
                    }
                }
            }
        }

        // For deleted nodes, we don't remove them from live yet
        // The revert task will handle cleanup when it completes

        // On first dispatch, add scope as blocker to all root-level live nodes
        if is_first_dispatch {
            self.state.add_scope_blocking_to_roots().await?;
            info!("Added scope blocking to root-level nodes");
        }

        Ok(())
    }

    /// Preview what would be dispatched without actually dispatching
    pub async fn preview(&self) -> DeltaDispatchResult<DispatchPreview> {
        let diff = self.generator.get_diff().await?;
        let tasks = if diff.is_empty() {
            vec![]
        } else {
            self.generator.generate_tasks().await.unwrap_or_default()
        };

        Ok(DispatchPreview {
            diff,
            tasks,
            has_existing_run: self.state.get_project_run().await?.is_some(),
        })
    }

    /// Get the current diff
    pub async fn get_diff(&self) -> DeltaDispatchResult<TreeDiff> {
        Ok(self.generator.get_diff().await?)
    }

    /// Get diff summary for display
    pub async fn get_diff_summary(&self) -> DeltaDispatchResult<String> {
        Ok(self.generator.get_diff_summary().await?)
    }

    /// Get the project run
    pub async fn get_project_run(&self) -> DeltaDispatchResult<Option<ProjectRun>> {
        Ok(self.state.get_project_run().await?)
    }

    /// Mark live node as complete (called when task finishes)
    pub async fn complete_live_node(
        &self,
        node_id: &str,
        success: bool,
        commit_sha: Option<&str>,
    ) -> DeltaDispatchResult<()> {
        let status = if success {
            LiveNodeStatus::Done
        } else {
            LiveNodeStatus::Failed
        };

        self.state
            .update_live_node_status(node_id, status, commit_sha)
            .await?;
        info!("Marked live node {} as {:?}", node_id, status);

        // Check if all nodes are done to pause the run
        self.check_run_completion().await?;

        Ok(())
    }

    /// Handle revert completion - remove the live node
    pub async fn complete_revert(&self, node_id: &str) -> DeltaDispatchResult<()> {
        self.state.delete_live_node(node_id).await?;
        info!("Deleted reverted live node: {}", node_id);

        self.check_run_completion().await?;

        Ok(())
    }

    /// Check if all tasks are done and pause the run
    async fn check_run_completion(&self) -> DeltaDispatchResult<()> {
        let live_nodes = self.state.get_live_nodes().await?;

        let all_done = live_nodes
            .iter()
            .all(|n| n.status == LiveNodeStatus::Done || n.status == LiveNodeStatus::Failed);

        if all_done && !live_nodes.is_empty() {
            info!("All live nodes complete, pausing run");
            self.state
                .update_project_run_status(ProjectRunStatus::Paused)
                .await?;
        }

        Ok(())
    }

    /// Get draft tree
    pub async fn get_draft_tree(&self) -> DeltaDispatchResult<Vec<DraftNodeTree>> {
        Ok(self.state.get_draft_tree().await?)
    }

    /// Get live tree
    pub async fn get_live_tree(&self) -> DeltaDispatchResult<Vec<LiveNodeTree>> {
        Ok(self.state.get_live_tree().await?)
    }

    /// Get underlying state
    pub fn state(&self) -> &DeltaState {
        &self.state
    }

    /// Get all board versions for this project
    pub async fn get_board_versions(&self) -> DeltaDispatchResult<Vec<BoardVersion>> {
        Ok(self.state.get_board_versions().await?)
    }

    /// Get the latest board version
    pub async fn get_latest_version(&self) -> DeltaDispatchResult<Option<BoardVersion>> {
        Ok(self.state.get_latest_version().await?)
    }

    /// Get a specific board version
    pub async fn get_board_version(&self, id: i64) -> DeltaDispatchResult<BoardVersion> {
        Ok(self.state.get_board_version(id).await?)
    }

    /// Create a delivery for the latest board version
    pub async fn create_delivery(&self, target_branch: &str) -> DeltaDispatchResult<Delivery> {
        let version = self
            .state
            .get_latest_version()
            .await?
            .ok_or_else(|| DispatchError::NoChanges)?;

        Ok(self
            .state
            .create_delivery(version.id, target_branch)
            .await?)
    }

    /// Get current delivery for this project
    pub async fn get_current_delivery(&self) -> DeltaDispatchResult<Option<Delivery>> {
        Ok(self.state.get_current_delivery().await?)
    }
}

/// Preview of what would be dispatched
#[derive(Debug, Clone)]
pub struct DispatchPreview {
    pub diff: TreeDiff,
    pub tasks: Vec<DeltaTask>,
    pub has_existing_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_service_creation() {
        let service = DeltaDispatchService::new(1, 0);
        assert_eq!(service.project_id, 1);
    }
}
