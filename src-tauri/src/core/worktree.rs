use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::core::delta::{BoardNode, BoardNodeTree, DeltaState};
use crate::core::route::Route;
use crate::core::CapabilityProfile;

pub const SYNC_PROJECT_TASK_TITLE: &str = "Sync project";
pub const SYNC_PROJECT_TASK_MARKER: &str = "Task kind: sync_project";
pub const SYNC_PROJECT_ASSIGNEE_KIND: &str = "channel";
pub const SYNC_PROJECT_ASSIGNEE_ID: &str = "main";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureSyncProjectTaskResult {
    pub route_id: i64,
    pub item: WorkItem,
    pub created: bool,
    pub requested: bool,
    pub should_prompt: bool,
    pub prompt: String,
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

fn sync_project_task_matches(node: &BoardNodeTree) -> bool {
    node.name
        .trim()
        .eq_ignore_ascii_case(SYNC_PROJECT_TASK_TITLE)
        || node.content.contains(SYNC_PROJECT_TASK_MARKER)
}

fn find_sync_project_item(nodes: &[BoardNodeTree]) -> Option<&BoardNodeTree> {
    for node in nodes {
        if sync_project_task_matches(node) {
            return Some(node);
        }
        if let Some(found) = find_sync_project_item(&node.children) {
            return Some(found);
        }
    }
    None
}

fn sync_project_task_description(project_name: &str, route_name: &str) -> String {
    format!(
        "{marker}\n\
         Survey the existing `{project}` codebase on route `{route}` and refresh Hirsel's understanding.\n\n\
         Expected phases:\n\
         - Survey the repo layout, stack, build/test/deploy setup, and major architecture clues.\n\
         - Synthesize the current understanding into the project focus view and retained docs/context.\n\
         - Propose the first useful child work items and surface open questions that need user input.\n\n\
         This task is rerunnable. Reopen or request it again whenever the codebase changed materially or Hirsel needs a fresh read.",
        marker = SYNC_PROJECT_TASK_MARKER,
        project = project_name,
        route = route_name,
    )
}

fn sync_project_prompt(project_name: &str, route_name: &str, item_id: &str) -> String {
    format!(
        "Sync this project for route `{route}`. Use work item `{item}` as the umbrella task. \
Survey the existing codebase and setup for `{project}`, update the project focus view and retained context to reflect what actually matters now, \
split follow-on child work items as needed, and surface open questions instead of guessing.",
        route = route_name,
        item = item_id,
        project = project_name,
    )
}

pub async fn ensure_sync_project_task(
    project_id: i64,
    route: &Route,
    project_name: &str,
    request_sync: bool,
    refresh: bool,
) -> anyhow::Result<EnsureSyncProjectTaskResult> {
    let state = DeltaState::with_route(project_id, route.id);
    let tree = state
        .get_tree()
        .await
        .with_context(|| format!("failed to load work tree for route {}", route.name))?;

    let mut created = false;
    let mut requested = false;

    let mut item = if let Some(existing) = find_sync_project_item(&tree) {
        state
            .get_node(&existing.id)
            .await
            .with_context(|| format!("failed to load sync task {}", existing.id))?
    } else {
        created = true;
        state
            .create_work_item(
                SYNC_PROJECT_TASK_TITLE,
                None,
                &sync_project_task_description(project_name, &route.name),
                &[],
            )
            .await
            .context("failed to create sync project work item")?
    };

    let is_terminal = matches!(item.status.as_str(), "done" | "validated" | "failed");
    if refresh && (item.archived_at.is_some() || is_terminal) {
        item = state
            .reopen_work_item(&item.id)
            .await
            .with_context(|| format!("failed to reopen sync task {}", item.id))?;
    }

    if request_sync {
        let already_routed = item.assigned_agent_kind.as_deref()
            == Some(SYNC_PROJECT_ASSIGNEE_KIND)
            && item.assigned_agent_id.as_deref() == Some(SYNC_PROJECT_ASSIGNEE_ID)
            && item.capability_profile == Some(CapabilityProfile::Channel);

        if !already_routed {
            item = state
                .assign_work_item(
                    &item.id,
                    Some(SYNC_PROJECT_ASSIGNEE_KIND),
                    Some(SYNC_PROJECT_ASSIGNEE_ID),
                    Some(CapabilityProfile::Channel),
                )
                .await
                .with_context(|| format!("failed to assign sync task {}", item.id))?;
            requested = true;
        }
    }

    let should_prompt = request_sync && (created || requested || refresh);

    let prompt = sync_project_prompt(project_name, &route.name, &item.id);

    Ok(EnsureSyncProjectTaskResult {
        route_id: route.id,
        item: item.into(),
        created,
        requested,
        should_prompt,
        prompt,
    })
}
