//! Delta dispatch types
//!
//! Types for the unified board with delta-based dispatch system:
//! - Draft nodes: user-editable tree
//! - Live nodes: dispatched/working state
//! - Delta types: implement, modify, revert

use serde::{Deserialize, Serialize};

// =============================================================================
// Node Types
// =============================================================================

/// Type of a node in the tree
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    #[default]
    Task,
    Eval,
    Project, // Root node representing the project
}

impl NodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Eval => "eval",
            Self::Project => "project",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "eval" => Self::Eval,
            "project" => Self::Project,
            _ => Self::Task,
        }
    }
}

/// Status of a live node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LiveNodeStatus {
    #[default]
    Pending,
    Working,
    Done,
    Failed,
}

impl LiveNodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Working => "working",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "working" => Self::Working,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// A node in the draft tree (user edits freely)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftNode {
    pub id: String,
    pub project_id: i64,
    pub parent_id: Option<String>,
    pub position: i32,
    pub name: String,
    pub node_type: NodeType,
    pub content: String,
    pub validates: Vec<String>, // For eval nodes
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Draft node tree (nested for frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftNodeTree {
    pub id: String,
    pub name: String,
    pub node_type: NodeType,
    pub content: String,
    pub validates: Vec<String>,
    pub children: Vec<DraftNodeTree>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

impl From<DraftNode> for DraftNodeTree {
    fn from(node: DraftNode) -> Self {
        Self {
            id: node.id,
            name: node.name,
            node_type: node.node_type,
            content: node.content,
            validates: node.validates,
            children: vec![],
            x: node.x,
            y: node.y,
        }
    }
}

/// A node in the live tree (dispatched state)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveNode {
    pub id: String,
    pub project_id: i64,
    pub draft_node_id: Option<String>, // Link to draft (null if deleted from draft)
    pub parent_id: Option<String>,
    pub position: i32,
    pub name: String,
    pub node_type: NodeType,
    pub content: String,
    pub status: LiveNodeStatus,
    pub validates: Vec<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub last_commit_sha: Option<String>,
}

/// Live node tree (nested for frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveNodeTree {
    pub id: String,
    pub draft_node_id: Option<String>,
    pub name: String,
    pub node_type: NodeType,
    pub content: String,
    pub status: LiveNodeStatus,
    pub validates: Vec<String>,
    pub children: Vec<LiveNodeTree>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub completed_at: Option<String>,
    pub last_commit_sha: Option<String>,
}

impl From<LiveNode> for LiveNodeTree {
    fn from(node: LiveNode) -> Self {
        Self {
            id: node.id,
            draft_node_id: node.draft_node_id,
            name: node.name,
            node_type: node.node_type,
            content: node.content,
            status: node.status,
            validates: node.validates,
            children: vec![],
            x: node.x,
            y: node.y,
            completed_at: node.completed_at,
            last_commit_sha: node.last_commit_sha,
        }
    }
}

// =============================================================================
// Delta Types
// =============================================================================

/// Type of delta operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaType {
    /// New node in draft that doesn't exist in live
    Implement,
    /// Existing node modified in draft
    Modify,
    /// Node deleted from draft but exists in live (needs revert)
    Revert,
}

impl DeltaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Implement => "implement",
            Self::Modify => "modify",
            Self::Revert => "revert",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "implement" => Some(Self::Implement),
            "modify" => Some(Self::Modify),
            "revert" => Some(Self::Revert),
            _ => None,
        }
    }
}

/// Status of a delta submission
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeltaStatus {
    #[default]
    Pending,
    Processing,
    Done,
    Failed,
}

impl DeltaStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "processing" => Self::Processing,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// A reference for context in delta tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    /// What kind of reference (e.g., "file", "task", "commit")
    pub ref_type: String,
    /// The reference value (e.g., file path, task ID, commit SHA)
    pub value: String,
    /// Optional description
    pub description: Option<String>,
}

/// A delta submission (LLM-generated task for dispatch)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaSubmission {
    pub id: i64,
    pub project_id: i64,
    pub batch_id: Option<i64>,
    pub delta_type: DeltaType,
    pub draft_node_id: Option<String>,
    pub live_node_id: Option<String>,
    pub name: String,
    pub description: String,
    pub priority: i32,
    pub status: DeltaStatus,
    pub refs: Vec<Reference>,
    pub created_at: String,
    pub processed_at: Option<String>,
}

/// A delta task generated by LLM (before DB insertion)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaTask {
    pub delta_type: DeltaType,
    pub name: String,
    pub description: String,
    pub draft_node_id: Option<String>,
    pub live_node_id: Option<String>,
    pub refs: Vec<Reference>,
    pub priority: i32,
}

// =============================================================================
// Diff Types
// =============================================================================

/// A node in a diff operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffNode {
    pub id: String,
    pub name: String,
    pub node_type: NodeType,
    pub content: String,
    pub validates: Vec<String>,
    pub parent_id: Option<String>,
}

impl From<&DraftNode> for DiffNode {
    fn from(node: &DraftNode) -> Self {
        Self {
            id: node.id.clone(),
            name: node.name.clone(),
            node_type: node.node_type,
            content: node.content.clone(),
            validates: node.validates.clone(),
            parent_id: node.parent_id.clone(),
        }
    }
}

impl From<&LiveNode> for DiffNode {
    fn from(node: &LiveNode) -> Self {
        Self {
            id: node.id.clone(),
            name: node.name.clone(),
            node_type: node.node_type,
            content: node.content.clone(),
            validates: node.validates.clone(),
            parent_id: node.parent_id.clone(),
        }
    }
}

/// A modified node with old and new state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModifiedNode {
    pub draft_node: DiffNode,
    pub live_node: DiffNode,
    /// What changed (for display)
    pub changes: Vec<String>,
}

/// Result of diffing draft vs live trees
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TreeDiff {
    /// Nodes in draft but not in live
    pub new_nodes: Vec<DiffNode>,
    /// Nodes in both but with different content
    pub modified_nodes: Vec<ModifiedNode>,
    /// Nodes in live but not in draft
    pub deleted_nodes: Vec<DiffNode>,
    /// Node IDs that are unchanged
    pub unchanged_ids: Vec<String>,
}

impl TreeDiff {
    pub fn is_empty(&self) -> bool {
        self.new_nodes.is_empty() && self.modified_nodes.is_empty() && self.deleted_nodes.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} new, {} modified, {} deleted",
            self.new_nodes.len(),
            self.modified_nodes.len(),
            self.deleted_nodes.len()
        )
    }
}

// =============================================================================
// Project Run Types
// =============================================================================

/// Status of a project's persistent run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRunStatus {
    #[default]
    Paused,
    Working,
    Failed,
}

impl ProjectRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Working => "working",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "working" => Self::Working,
            "failed" => Self::Failed,
            _ => Self::Paused,
        }
    }
}

/// A persistent run for a project
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRun {
    pub id: i64,
    pub project_id: i64,
    pub run_name: String,
    pub status: ProjectRunStatus,
    pub created_at: String,
    pub last_dispatch_at: Option<String>,
}

// =============================================================================
// Request Types
// =============================================================================

/// Request to create a draft node
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDraftNodeRequest {
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub node_type: NodeType,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub validates: Vec<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// Request to update a draft node
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDraftNodeRequest {
    pub name: Option<String>,
    pub content: Option<String>,
    pub validates: Option<Vec<String>>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// Result of a dispatch operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchResult {
    pub run_name: String,
    pub batch_id: i64,
    pub delta_count: usize,
    pub diff_summary: String,
}
