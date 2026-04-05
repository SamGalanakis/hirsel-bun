use serde::Serialize;
use serde_json::Value;

use crate::backend::project::{Project, ProjectPreparationStep, ProjectRuntimePreparation};
use crate::backend::shepherd_runtime::{self, ShepherdMessageChunk};
use crate::backend::{ShepherdChatMessage, ShepherdThread};

#[derive(Serialize)]
pub struct ApiProject {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub sandbox_image: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ApiPreparationStep {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: Option<String>,
    pub progress: Option<f64>,
}

#[derive(Serialize)]
pub struct ApiProjectPreparation {
    pub project: ApiProject,
    pub worker_image: String,
    pub status: String,
    pub headline: String,
    pub detail: Option<String>,
    pub progress: f64,
    pub steps: Vec<ApiPreparationStep>,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ApiChatMessage {
    pub id: i64,
    pub role: String,
    pub chunks_json: String,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct ApiLiveTurn {
    pub chunks_json: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ApiSession {
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Serialize)]
pub struct ApiScopeActivity {
    pub session: Option<ApiSession>,
    pub live_turn: Option<ApiLiveTurn>,
    pub has_active_turn: bool,
}

#[derive(Serialize)]
pub struct ApiThread {
    pub id: String,
    pub project_id: i64,
    pub title: String,
    pub objective: String,
    pub summary: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
}

#[derive(Serialize)]
pub struct ApiPlanProgress {
    pub completed: usize,
    pub total: usize,
}

pub fn to_api_project(project: &Project) -> ApiProject {
    ApiProject {
        id: project.id,
        name: project.name.clone(),
        description: project.description.clone(),
        sandbox_image: project.sandbox_image.clone(),
        created_at: project.created_at.clone(),
    }
}

pub fn to_api_preparation_step(step: &ProjectPreparationStep) -> ApiPreparationStep {
    ApiPreparationStep {
        id: step.id.clone(),
        label: step.label.clone(),
        status: step.status.clone(),
        detail: step.detail.clone(),
        progress: step.progress,
    }
}

pub fn to_api_project_preparation(
    project: &Project,
    preparation: &ProjectRuntimePreparation,
    worker_image: String,
) -> ApiProjectPreparation {
    ApiProjectPreparation {
        project: to_api_project(project),
        worker_image,
        status: preparation.status.clone(),
        headline: preparation.headline.clone(),
        detail: preparation.detail.clone(),
        progress: preparation.progress,
        steps: preparation
            .steps
            .iter()
            .map(to_api_preparation_step)
            .collect(),
        started_at: preparation.started_at.clone(),
        updated_at: preparation.updated_at.clone(),
    }
}

pub fn to_api_message(message: &ShepherdChatMessage) -> ApiChatMessage {
    ApiChatMessage {
        id: message.id,
        role: message.role.clone(),
        chunks_json: message.chunks_json.clone(),
        timestamp: message.timestamp.clone(),
    }
}

pub fn to_api_activity(activity: &shepherd_runtime::ShepherdScopeActivity) -> ApiScopeActivity {
    ApiScopeActivity {
        session: activity.session.as_ref().map(|session| ApiSession {
            status: session.status.clone(),
            last_error: session.last_error.clone(),
        }),
        live_turn: activity.live_turn.as_ref().map(|turn| ApiLiveTurn {
            chunks_json: turn.chunks_json.clone(),
            status: turn.status.clone(),
            updated_at: turn.updated_at.clone(),
        }),
        has_active_turn: activity.has_active_turn,
    }
}

pub fn to_api_thread(thread: &ShepherdThread) -> ApiThread {
    ApiThread {
        id: thread.id.clone(),
        project_id: thread.project_id,
        title: thread.title.clone(),
        objective: thread.objective.clone(),
        summary: thread.summary.clone(),
        status: thread.status.clone(),
        created_at: thread.created_at.clone(),
        updated_at: thread.updated_at.clone(),
        last_activity_at: thread.last_activity_at.clone(),
    }
}

pub fn extract_latest_plan(messages: &[ShepherdChatMessage]) -> Option<Value> {
    for message in messages.iter().rev() {
        let Ok(chunks) = serde_json::from_str::<Vec<ShepherdMessageChunk>>(&message.chunks_json)
        else {
            continue;
        };
        for chunk in chunks.into_iter().rev() {
            let ShepherdMessageChunk::Tool { input, .. } = chunk else {
                continue;
            };
            let Some(input) = input else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&input) else {
                continue;
            };
            if parsed
                .get("plan")
                .and_then(|value| value.as_array())
                .is_some()
            {
                return Some(parsed);
            }
        }
    }
    None
}

pub fn plan_progress_from_messages(messages: &[ShepherdChatMessage]) -> Option<ApiPlanProgress> {
    let plan = extract_latest_plan(messages)?;
    let steps = plan.get("plan")?.as_array()?;
    let total = steps.len();
    let completed = steps
        .iter()
        .filter(|step| step.get("status").and_then(|value| value.as_str()) == Some("completed"))
        .count();
    Some(ApiPlanProgress { completed, total })
}
