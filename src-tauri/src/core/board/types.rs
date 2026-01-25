//! Agent-facing types for board JSON files
//!
//! These types represent the projection of board data that agents can read/write.
//! They exclude UI-specific fields like timestamps but include optional positioning.

use serde::{Deserialize, Serialize};

/// Agent view of a task (minimal projection of DB task)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskView {
    pub id: String,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub status: String,
    /// Task IDs this task is blocked by
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    /// Nested subtasks
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtasks: Vec<AgentTaskView>,
}

/// Agent view of a row's eval section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvalView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criteria: Option<String>,
    #[serde(default)]
    pub status: String,
}

/// Agent view of a row (trifecta: spec | tasks | eval)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRowView {
    pub id: String,
    /// The spec content (markdown)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Tasks for this row
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<AgentTaskView>,
    /// Eval section
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval: Option<AgentEvalView>,
}

/// Agent view of an island (feature container)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIslandView {
    pub id: String,
    pub title: String,
    /// Optional X position (agent can suggest, bounded on import)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Optional Y position (agent can suggest, bounded on import)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Rows within the island (ordered by position)
    #[serde(default)]
    pub rows: Vec<AgentRowView>,
}

/// Result of a sync operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Number of changes applied
    pub changes: usize,
    /// Islands that were added
    pub added: Vec<String>,
    /// Islands that were updated
    pub updated: Vec<String>,
    /// Islands that were deleted
    pub deleted: Vec<String>,
}

impl Default for SyncResult {
    fn default() -> Self {
        Self {
            changes: 0,
            added: vec![],
            updated: vec![],
            deleted: vec![],
        }
    }
}
