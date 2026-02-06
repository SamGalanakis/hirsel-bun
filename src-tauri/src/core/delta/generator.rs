//! Delta task generator using LLM
//!
//! Generates delta tasks (implement, modify, revert) from a tree diff
//! using an LLM to create meaningful task descriptions.

use tracing::info;

use super::diff::DiffService;
use super::types::*;

/// Error type for generator operations
#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("State error: {0}")]
    State(#[from] super::state::DeltaStateError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("No changes to generate tasks for")]
    NoChanges,
}

pub type GeneratorResult<T> = Result<T, GeneratorError>;

/// Delta task generator
pub struct DeltaGenerator {
    diff_service: DiffService,
}

impl DeltaGenerator {
    /// Create a new generator for a project route
    pub fn new(project_id: i64, route_id: i64) -> Self {
        Self {
            diff_service: DiffService::new(project_id, route_id),
        }
    }

    /// Generate delta tasks from the current diff
    ///
    /// For now, this uses a simple rule-based approach.
    /// In production, this could call an LLM for richer task descriptions.
    pub async fn generate_tasks(&self) -> GeneratorResult<Vec<DeltaTask>> {
        let diff = self.diff_service.compute_diff().await?;

        if diff.is_empty() {
            return Err(GeneratorError::NoChanges);
        }

        let mut tasks = Vec::new();

        // Generate implement tasks for new nodes
        for node in &diff.new_nodes {
            tasks.push(self.generate_implement_task(node));
        }

        // Generate modify tasks for modified nodes
        for modified in &diff.modified_nodes {
            tasks.push(self.generate_modify_task(modified));
        }

        // Generate revert tasks for deleted nodes
        for node in &diff.deleted_nodes {
            tasks.push(self.generate_revert_task(node));
        }

        // Sort by priority (higher priority first)
        tasks.sort_by(|a, b| b.priority.cmp(&a.priority));

        info!(
            "Generated {} delta tasks: {} implement, {} modify, {} revert",
            tasks.len(),
            diff.new_nodes.len(),
            diff.modified_nodes.len(),
            diff.deleted_nodes.len()
        );

        Ok(tasks)
    }

    /// Generate an implement task for a new node
    fn generate_implement_task(&self, node: &DiffNode) -> DeltaTask {
        let description = if node.content.is_empty() {
            format!("Implement new {} '{}'", node.node_type.as_str(), node.name)
        } else {
            // Use the node content as the task description
            format!(
                "Implement new {} '{}':\n\n{}",
                node.node_type.as_str(),
                node.name,
                node.content
            )
        };

        DeltaTask {
            delta_type: DeltaType::Implement,
            name: format!("Implement: {}", node.name),
            description,
            draft_node_id: Some(node.id.clone()),
            // Live node will be created with the same ID as draft node
            live_node_id: Some(node.id.clone()),
            refs: vec![],
            priority: self.calculate_priority(node, DeltaType::Implement),
        }
    }

    /// Generate a modify task for a modified node
    fn generate_modify_task(&self, modified: &ModifiedNode) -> DeltaTask {
        let changes_summary = modified.changes.join(", ");

        let description = format!(
            "Update {} '{}' with changes:\n\n{}\n\nNew spec:\n\n{}",
            modified.draft_node.node_type.as_str(),
            modified.draft_node.name,
            changes_summary,
            modified.draft_node.content
        );

        DeltaTask {
            delta_type: DeltaType::Modify,
            name: format!("Modify: {}", modified.draft_node.name),
            description,
            draft_node_id: Some(modified.draft_node.id.clone()),
            live_node_id: Some(modified.live_node.id.clone()),
            refs: vec![],
            priority: self.calculate_priority(&modified.draft_node, DeltaType::Modify),
        }
    }

    /// Generate a revert task for a deleted node
    fn generate_revert_task(&self, node: &DiffNode) -> DeltaTask {
        let description = format!(
            "Revert {} '{}' - this was removed from the spec and any implementation should be undone.\n\nOriginal content:\n\n{}",
            node.node_type.as_str(),
            node.name,
            node.content
        );

        DeltaTask {
            delta_type: DeltaType::Revert,
            name: format!("Revert: {}", node.name),
            description,
            draft_node_id: None,
            live_node_id: Some(node.id.clone()),
            refs: vec![],
            priority: self.calculate_priority(node, DeltaType::Revert),
        }
    }

    /// Calculate priority based on node characteristics
    fn calculate_priority(&self, node: &DiffNode, delta_type: DeltaType) -> i32 {
        let mut priority = 0;

        // Eval nodes have higher priority (they need to run after work tasks)
        if node.node_type == NodeType::Eval {
            priority -= 10; // Lower priority = runs later
        }

        // Reverts should generally run first to clean up
        if delta_type == DeltaType::Revert {
            priority += 5;
        }

        // Root nodes (no parent) have higher priority
        if node.parent_id.is_none() {
            priority += 2;
        }

        priority
    }

    /// Get the diff service
    pub fn diff_service(&self) -> &DiffService {
        &self.diff_service
    }

    /// Compute and get the current diff
    pub async fn get_diff(&self) -> GeneratorResult<TreeDiff> {
        Ok(self.diff_service.compute_diff().await?)
    }

    /// Generate diff summary for display
    pub async fn get_diff_summary(&self) -> GeneratorResult<String> {
        let diff = self.diff_service.compute_diff().await?;
        Ok(self.diff_service.generate_summary(&diff))
    }
}
