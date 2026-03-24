//! Route-scoped worker event queries.

use crate::core::api_types::{WorkerEventResponse, WorkerEventsResponse};
use crate::core::state::SQLiteState;

use super::workers::resolve_route_runtime_name;
use super::ResultExt;

fn map_worker_event(event: crate::core::state::WorkerEvent) -> WorkerEventResponse {
    WorkerEventResponse {
        id: event.id,
        worker_name: event.worker_name,
        event_type: event.event_type.as_str().to_string(),
        timestamp: event.timestamp,
        content: event.content,
        tool_call_id: event.tool_call_id,
        tool_title: event.tool_title,
        tool_kind: event.tool_kind,
        tool_status: event.tool_status.map(|status| status.as_str().to_string()),
        tool_input: event.tool_input,
        tool_output: event.tool_output,
    }
}

pub async fn get_route_worker_events(
    project_id: i64,
    route_id: i64,
    worker_name: String,
    after_id: Option<i64>,
    limit: Option<i64>,
) -> Result<WorkerEventsResponse, String> {
    let runtime_name = match resolve_route_runtime_name(project_id, route_id).await? {
        Some(runtime_name) => runtime_name,
        None => {
            return Ok(WorkerEventsResponse {
                events: Vec::new(),
                last_id: None,
                worker_status: None,
            });
        }
    };
    let state = SQLiteState::new(&runtime_name).await.str_err()?;
    let events = state
        .get_worker_events(&worker_name, after_id, limit.unwrap_or(1000))
        .await
        .context("Failed to get worker events")?;
    let worker_status = state
        .get_worker(&worker_name)
        .await
        .ok()
        .flatten()
        .map(|worker| worker.status.as_str().to_string());

    Ok(WorkerEventsResponse {
        last_id: events.last().map(|event| event.id),
        events: events.into_iter().map(map_worker_event).collect(),
        worker_status,
    })
}
