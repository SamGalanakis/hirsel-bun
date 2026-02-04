//! Diff service for computing differences between draft and live trees
//!
//! Produces a structured diff that identifies:
//! - New nodes (in draft but not in live)
//! - Modified nodes (content changed)
//! - Deleted nodes (in live but not in draft)
//! - Unchanged nodes

use std::collections::{HashMap, HashSet};

use super::state::{DeltaState, DeltaStateResult};
use super::types::*;

/// Service for computing tree diffs
pub struct DiffService {
    state: DeltaState,
}

impl DiffService {
    /// Create a new diff service for a project (uses route_id = 0 for backwards compatibility)
    pub fn new(project_id: i64) -> Self {
        Self::with_route(project_id, 0)
    }

    /// Create a new diff service for a project route
    pub fn with_route(project_id: i64, route_id: i64) -> Self {
        Self {
            state: DeltaState::with_route(project_id, route_id),
        }
    }

    /// Compute the diff between draft and live trees
    pub async fn compute_diff(&self) -> DeltaStateResult<TreeDiff> {
        let draft_nodes = self.state.get_draft_nodes().await?;
        let live_nodes = self.state.get_live_nodes().await?;

        // Build lookup maps
        let draft_by_id: HashMap<&str, &DraftNode> =
            draft_nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let live_by_id: HashMap<&str, &LiveNode> =
            live_nodes.iter().map(|n| (n.id.as_str(), n)).collect();

        let draft_ids: HashSet<&str> = draft_by_id.keys().copied().collect();
        let live_ids: HashSet<&str> = live_by_id.keys().copied().collect();

        let mut diff = TreeDiff::default();

        // Find new nodes (in draft but not in live)
        for id in draft_ids.difference(&live_ids) {
            if let Some(draft) = draft_by_id.get(id) {
                diff.new_nodes.push(DiffNode::from(*draft));
            }
        }

        // Find deleted nodes (in live but not in draft)
        for id in live_ids.difference(&draft_ids) {
            if let Some(live) = live_by_id.get(id) {
                diff.deleted_nodes.push(DiffNode::from(*live));
            }
        }

        // Find modified and unchanged nodes
        for id in draft_ids.intersection(&live_ids) {
            let draft = draft_by_id.get(id).unwrap();
            let live = live_by_id.get(id).unwrap();

            let changes = self.detect_changes(draft, live);
            if changes.is_empty() {
                diff.unchanged_ids.push(id.to_string());
            } else {
                diff.modified_nodes.push(ModifiedNode {
                    draft_node: DiffNode::from(*draft),
                    live_node: DiffNode::from(*live),
                    changes,
                });
            }
        }

        Ok(diff)
    }

    /// Detect what changed between draft and live versions of a node
    fn detect_changes(&self, draft: &DraftNode, live: &LiveNode) -> Vec<String> {
        let mut changes = Vec::new();

        if draft.name != live.name {
            changes.push(format!("name: '{}' -> '{}'", live.name, draft.name));
        }

        if draft.content != live.content {
            // Don't include full content in change description
            let draft_len = draft.content.len();
            let live_len = live.content.len();
            changes.push(format!(
                "content: {} chars -> {} chars",
                live_len, draft_len
            ));
        }

        if draft.node_type != live.node_type {
            changes.push(format!(
                "type: {} -> {}",
                live.node_type.as_str(),
                draft.node_type.as_str()
            ));
        }

        if draft.validates != live.validates {
            changes.push(format!(
                "validates: {:?} -> {:?}",
                live.validates, draft.validates
            ));
        }

        if draft.blocked_by != live.blocked_by {
            changes.push(format!(
                "blocked_by: {:?} -> {:?}",
                live.blocked_by, draft.blocked_by
            ));
        }

        if draft.parent_id != live.parent_id {
            changes.push(format!(
                "parent: {:?} -> {:?}",
                live.parent_id, draft.parent_id
            ));
        }

        changes
    }

    /// Generate a human-readable summary of the diff for LLM context
    pub fn generate_summary(&self, diff: &TreeDiff) -> String {
        let mut summary = String::new();

        if diff.is_empty() {
            return "No changes between draft and live trees.".to_string();
        }

        summary.push_str("## Tree Diff Summary\n\n");

        if !diff.new_nodes.is_empty() {
            summary.push_str("### New Nodes (to implement)\n\n");
            for node in &diff.new_nodes {
                summary.push_str(&format!(
                    "- **{}** (`{}`, type: {})\n",
                    node.name,
                    node.id,
                    node.node_type.as_str()
                ));
                if !node.content.is_empty() {
                    let preview: String = node
                        .content
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(80)
                        .collect();
                    summary.push_str(&format!("  > {}\n", preview));
                }
            }
            summary.push('\n');
        }

        if !diff.modified_nodes.is_empty() {
            summary.push_str("### Modified Nodes (to update)\n\n");
            for modified in &diff.modified_nodes {
                summary.push_str(&format!(
                    "- **{}** (`{}`)\n",
                    modified.draft_node.name, modified.draft_node.id
                ));
                for change in &modified.changes {
                    summary.push_str(&format!("  - {}\n", change));
                }
            }
            summary.push('\n');
        }

        if !diff.deleted_nodes.is_empty() {
            summary.push_str("### Deleted Nodes (to revert)\n\n");
            for node in &diff.deleted_nodes {
                summary.push_str(&format!(
                    "- **{}** (`{}`, status: {})\n",
                    node.name, node.id, "live"
                ));
            }
            summary.push('\n');
        }

        summary.push_str(&format!(
            "\n**Total:** {} new, {} modified, {} deleted, {} unchanged\n",
            diff.new_nodes.len(),
            diff.modified_nodes.len(),
            diff.deleted_nodes.len(),
            diff.unchanged_ids.len()
        ));

        summary
    }

    /// Get the underlying state
    pub fn state(&self) -> &DeltaState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_diff() {
        let diff = TreeDiff::default();
        assert!(diff.is_empty());
        assert_eq!(diff.summary(), "0 new, 0 modified, 0 deleted");
    }

    #[test]
    fn test_diff_summary() {
        let diff = TreeDiff {
            new_nodes: vec![DiffNode {
                id: "new-1".to_string(),
                name: "New Task".to_string(),
                node_type: NodeType::Task,
                content: "Do something".to_string(),
                validates: vec![],
                blocked_by: vec![],
                parent_id: None,
            }],
            modified_nodes: vec![],
            deleted_nodes: vec![],
            unchanged_ids: vec!["unchanged-1".to_string()],
        };

        assert!(!diff.is_empty());
        assert_eq!(diff.summary(), "1 new, 0 modified, 0 deleted");
    }
}
