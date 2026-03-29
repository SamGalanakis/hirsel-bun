use serde::{Deserialize, Serialize};

use crate::backend::delta::{BoardNode, BoardNodeTree};
use crate::backend::CapabilityProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRef {
    pub kind: String,
    pub id: String,
    pub capability_profile: Option<CapabilityProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: String,
    pub status: String,
    pub blocked_by: Vec<String>,
    pub claimed_by: Option<String>,
    pub completed_by: Option<String>,
    pub completed_at: Option<String>,
    pub assignee: Option<AgentRef>,
    pub capability_profile: Option<CapabilityProfile>,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemTree {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: String,
    pub status: String,
    pub blocked_by: Vec<String>,
    pub claimed_by: Option<String>,
    pub completed_by: Option<String>,
    pub completed_at: Option<String>,
    pub assignee: Option<AgentRef>,
    pub capability_profile: Option<CapabilityProfile>,
    pub archived_at: Option<String>,
    pub children: Vec<WorkItemTree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTreeSnapshot {
    pub route_id: i64,
    pub tree: Vec<WorkItemTree>,
    pub generation: i64,
}

impl From<BoardNode> for WorkItem {
    fn from(node: BoardNode) -> Self {
        Self {
            id: node.id,
            parent_id: node.parent_id,
            title: node.name,
            description: node.content,
            status: node.status.as_str().to_string(),
            blocked_by: node.blocked_by,
            claimed_by: node.claimed_by,
            completed_by: node.completed_by,
            completed_at: node.completed_at,
            assignee: node
                .assigned_agent_kind
                .zip(node.assigned_agent_id)
                .map(|(kind, id)| AgentRef {
                    kind,
                    id,
                    capability_profile: node.capability_profile,
                }),
            capability_profile: node.capability_profile,
            archived_at: node.archived_at,
        }
    }
}

impl From<BoardNodeTree> for WorkItemTree {
    fn from(node: BoardNodeTree) -> Self {
        Self {
            id: node.id,
            parent_id: node.parent_id,
            title: node.name,
            description: node.content,
            status: node.status.as_str().to_string(),
            blocked_by: node.blocked_by,
            claimed_by: node.claimed_by,
            completed_by: node.completed_by,
            completed_at: node.completed_at,
            assignee: node
                .assigned_agent_kind
                .zip(node.assigned_agent_id)
                .map(|(kind, id)| AgentRef {
                    kind,
                    id,
                    capability_profile: node.capability_profile,
                }),
            capability_profile: node.capability_profile,
            archived_at: node.archived_at,
            children: node.children.into_iter().map(WorkItemTree::from).collect(),
        }
    }
}
