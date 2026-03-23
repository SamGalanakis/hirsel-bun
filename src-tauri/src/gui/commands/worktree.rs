//! Route work-tree commands.

use serde::{Deserialize, Serialize};

use super::ResultExt;
use crate::core::{
    ensure_sync_project_task, CapabilityProfile, DeltaDispatchService, DeltaState,
    EnsureSyncProjectTaskResult, ProjectStore, Route, RouteStore, WorkItem, WorkItemTree,
    WorkTreeSnapshot,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitWorkItemRequest {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
}

fn filter_archived(
    nodes: Vec<crate::core::delta::BoardNodeTree>,
) -> Vec<crate::core::delta::BoardNodeTree> {
    nodes
        .into_iter()
        .filter(|node| node.archived_at.is_none())
        .map(|mut node| {
            node.children = filter_archived(node.children);
            node
        })
        .collect()
}

fn visible_work_tree(nodes: Vec<crate::core::delta::BoardNodeTree>) -> Vec<WorkItemTree> {
    let active_nodes = filter_archived(nodes);
    if active_nodes.len() == 1 && active_nodes[0].parent_id.is_none() {
        return active_nodes[0]
            .children
            .clone()
            .into_iter()
            .map(WorkItemTree::from)
            .collect();
    }
    active_nodes.into_iter().map(WorkItemTree::from).collect()
}

async fn resolve_target_route(project_id: i64, route_id: Option<i64>) -> Result<Route, String> {
    let route_store = RouteStore::new(project_id).await.str_err()?;

    if let Some(route_id) = route_id {
        return route_store.get_route(route_id).await.str_err();
    }

    let project_store = ProjectStore::open().await.str_err()?;
    let project = project_store.get_project(project_id).await.str_err()?;

    if let Some(active_route_id) = project.active_route_id {
        if let Ok(route) = route_store.get_route(active_route_id).await {
            return Ok(route);
        }
    }

    route_store
        .list_routes()
        .await
        .str_err()?
        .into_iter()
        .next()
        .ok_or_else(|| "No routes exist for this project".to_string())
}

#[tracing::instrument]
#[tauri::command]
pub async fn get_route_work_tree(
    project_id: i64,
    route_id: i64,
) -> Result<WorkTreeSnapshot, String> {
    let dispatch = DeltaDispatchService::new(project_id, route_id);
    let state = DeltaState::with_route(project_id, route_id);
    let tree = dispatch.get_tree().await.str_err()?;
    let generation = state.tree_generation().await.str_err()?;

    Ok(WorkTreeSnapshot {
        route_id,
        tree: visible_work_tree(tree),
        generation,
    })
}

#[tracing::instrument]
#[tauri::command]
pub async fn create_work_item(
    project_id: i64,
    route_id: i64,
    parent_id: Option<String>,
    title: String,
    description: Option<String>,
    blocked_by: Option<Vec<String>>,
) -> Result<WorkItem, String> {
    let state = DeltaState::with_route(project_id, route_id);
    let item = state
        .create_work_item(
            &title,
            parent_id.as_deref(),
            description.as_deref().unwrap_or(""),
            blocked_by.as_deref().unwrap_or(&[]),
        )
        .await
        .str_err()?;
    Ok(item.into())
}

#[tracing::instrument]
#[tauri::command]
pub async fn reparent_work_item(
    project_id: i64,
    route_id: i64,
    item_id: String,
    new_parent_id: Option<String>,
    new_position: i32,
) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state
        .move_node(&item_id, new_parent_id.as_deref(), new_position)
        .await
        .str_err()
}

#[tracing::instrument]
#[tauri::command]
pub async fn split_work_item(
    project_id: i64,
    route_id: i64,
    item_id: String,
    items: Vec<SplitWorkItemRequest>,
) -> Result<Vec<WorkItem>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    let mut created = Vec::with_capacity(items.len());
    for item in items {
        let created_item = state
            .create_work_item(
                &item.title,
                Some(&item_id),
                item.description.as_deref().unwrap_or(""),
                &[],
            )
            .await
            .str_err()?;
        created.push(created_item.into());
    }
    Ok(created)
}

#[tracing::instrument]
#[tauri::command]
pub async fn assign_work_item(
    project_id: i64,
    route_id: i64,
    item_id: String,
    agent_kind: Option<String>,
    agent_id: Option<String>,
    capability_profile: Option<CapabilityProfile>,
) -> Result<WorkItem, String> {
    let state = DeltaState::with_route(project_id, route_id);
    let item = state
        .assign_work_item(
            &item_id,
            agent_kind.as_deref(),
            agent_id.as_deref(),
            capability_profile,
        )
        .await
        .str_err()?;
    Ok(item.into())
}

#[tracing::instrument]
#[tauri::command]
pub async fn reopen_work_item(
    project_id: i64,
    route_id: i64,
    item_id: String,
) -> Result<WorkItem, String> {
    let state = DeltaState::with_route(project_id, route_id);
    let item = state.reopen_work_item(&item_id).await.str_err()?;
    Ok(item.into())
}

#[tracing::instrument]
#[tauri::command]
pub async fn archive_work_item(
    project_id: i64,
    route_id: i64,
    item_id: String,
) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.archive_work_item(&item_id).await.str_err()
}

#[tracing::instrument]
#[tauri::command]
pub async fn ensure_sync_project_task_cmd(
    project_id: i64,
    route_id: Option<i64>,
    request_sync: Option<bool>,
    refresh: Option<bool>,
) -> Result<EnsureSyncProjectTaskResult, String> {
    let project_store = ProjectStore::open().await.str_err()?;
    let project = project_store.get_project(project_id).await.str_err()?;
    let route = resolve_target_route(project_id, route_id).await?;

    ensure_sync_project_task(
        project_id,
        &route,
        &project.name,
        request_sync.unwrap_or(false),
        refresh.unwrap_or(false),
    )
    .await
    .str_err()
}
