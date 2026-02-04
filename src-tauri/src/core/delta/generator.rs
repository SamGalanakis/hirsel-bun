//! Delta task generator using LLM
//!
//! Generates delta tasks (implement, modify, revert) from a tree diff
//! using an LLM to create meaningful task descriptions.

use serde::{Deserialize, Serialize};
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

/// LLM response format for delta tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmDeltaResponse {
    tasks: Vec<LlmDeltaTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmDeltaTask {
    delta_type: String,
    name: String,
    description: String,
    node_id: Option<String>,
    refs: Option<Vec<LlmReference>>,
    priority: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmReference {
    ref_type: String,
    value: String,
    description: Option<String>,
}

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

/// Build the LLM prompt for delta task generation
///
/// This is a more sophisticated approach that could be used
/// to get better task descriptions from an LLM.
pub fn build_llm_prompt(diff: &TreeDiff, project_context: Option<&str>) -> String {
    let mut prompt = String::new();

    prompt.push_str("You are generating delta tasks for a software project.\n\n");

    if let Some(context) = project_context {
        prompt.push_str("## Project Context\n\n");
        prompt.push_str(context);
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Changes\n\n");

    if !diff.new_nodes.is_empty() {
        prompt.push_str("### New Items to Implement\n\n");
        for node in &diff.new_nodes {
            prompt.push_str(&format!(
                "**{}** ({})\n",
                node.name,
                node.node_type.as_str()
            ));
            if !node.content.is_empty() {
                prompt.push_str(&format!("```\n{}\n```\n", node.content));
            }
            prompt.push('\n');
        }
    }

    if !diff.modified_nodes.is_empty() {
        prompt.push_str("### Modified Items\n\n");
        for modified in &diff.modified_nodes {
            prompt.push_str(&format!("**{}**\n", modified.draft_node.name));
            prompt.push_str("Changes:\n");
            for change in &modified.changes {
                prompt.push_str(&format!("- {}\n", change));
            }
            prompt.push_str("\nNew content:\n");
            prompt.push_str(&format!("```\n{}\n```\n\n", modified.draft_node.content));
        }
    }

    if !diff.deleted_nodes.is_empty() {
        prompt.push_str("### Removed Items (to revert)\n\n");
        for node in &diff.deleted_nodes {
            prompt.push_str(&format!(
                "**{}** ({})\n",
                node.name,
                node.node_type.as_str()
            ));
            prompt.push('\n');
        }
    }

    prompt.push_str(
        r#"
## Instructions

Generate a JSON response with tasks for each change. Format:

```json
{
  "tasks": [
    {
      "delta_type": "implement" | "modify" | "revert",
      "name": "Short task name",
      "description": "Detailed description of what to do",
      "node_id": "id of the node",
      "priority": 0,
      "refs": [
        {"ref_type": "file", "value": "path/to/file.rs", "description": "Related file"}
      ]
    }
  ]
}
```

Generate clear, actionable task descriptions that a developer can follow.
"#,
    );

    prompt
}

/// Parse LLM response into delta tasks
pub fn parse_llm_response(response: &str, _diff: &TreeDiff) -> GeneratorResult<Vec<DeltaTask>> {
    // Try to extract JSON from the response
    let json_str = extract_json(response)
        .ok_or_else(|| GeneratorError::Llm("No valid JSON found in LLM response".to_string()))?;

    let llm_response: LlmDeltaResponse = serde_json::from_str(json_str)?;

    let mut tasks = Vec::new();

    for llm_task in llm_response.tasks {
        let delta_type = DeltaType::from_str(&llm_task.delta_type).ok_or_else(|| {
            GeneratorError::Llm(format!("Invalid delta_type: {}", llm_task.delta_type))
        })?;

        let (draft_node_id, live_node_id) = match delta_type {
            DeltaType::Implement => (llm_task.node_id.clone(), None),
            DeltaType::Modify => {
                let node_id = llm_task.node_id.clone();
                (node_id.clone(), node_id)
            }
            DeltaType::Revert => (None, llm_task.node_id.clone()),
        };

        let refs = llm_task
            .refs
            .unwrap_or_default()
            .into_iter()
            .map(|r| Reference {
                ref_type: r.ref_type,
                value: r.value,
                description: r.description,
            })
            .collect();

        tasks.push(DeltaTask {
            delta_type,
            name: llm_task.name,
            description: llm_task.description,
            draft_node_id,
            live_node_id,
            refs,
            priority: llm_task.priority.unwrap_or(0),
        });
    }

    Ok(tasks)
}

/// Extract JSON from a string that might contain markdown code blocks
fn extract_json(s: &str) -> Option<&str> {
    // Try to find JSON in code blocks
    if let Some(start) = s.find("```json") {
        let start = start + 7;
        if let Some(end) = s[start..].find("```") {
            return Some(s[start..start + end].trim());
        }
    }

    // Try plain code blocks
    if let Some(start) = s.find("```") {
        let start = start + 3;
        // Skip language identifier if present
        let start = s[start..]
            .find('\n')
            .map(|i| start + i + 1)
            .unwrap_or(start);
        if let Some(end) = s[start..].find("```") {
            return Some(s[start..start + end].trim());
        }
    }

    // Try to find raw JSON
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            if end > start {
                return Some(&s[start..=end]);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_code_block() {
        let input = r#"Here's the response:

```json
{"tasks": []}
```

Done!"#;
        assert_eq!(extract_json(input), Some(r#"{"tasks": []}"#));
    }

    #[test]
    fn test_extract_json_raw() {
        let input = r#"{"tasks": [{"name": "test"}]}"#;
        assert_eq!(
            extract_json(input),
            Some(r#"{"tasks": [{"name": "test"}]}"#)
        );
    }

    #[test]
    fn test_build_prompt() {
        let diff = TreeDiff {
            new_nodes: vec![DiffNode {
                id: "test".to_string(),
                name: "Test Task".to_string(),
                node_type: NodeType::Task,
                content: "Do the thing".to_string(),
                validates: vec![],
                blocked_by: vec![],
                parent_id: None,
            }],
            ..Default::default()
        };

        let prompt = build_llm_prompt(&diff, None);
        assert!(prompt.contains("Test Task"));
        assert!(prompt.contains("Do the thing"));
    }
}
