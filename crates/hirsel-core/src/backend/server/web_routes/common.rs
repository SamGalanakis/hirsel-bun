use serde::Serialize;

use crate::backend::plans::{self, PlanSnapshot};
use crate::backend::project::{Project, ProjectWorkspaceEntry};
use crate::backend::shepherd_runtime;
use crate::backend::{ShepherdChatMessage, ShepherdThread};

#[derive(Serialize)]
pub struct ApiProject {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub workspaces: Vec<ProjectWorkspaceEntry>,
    pub shepherd_cwd: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ApiChatMessage {
    pub id: i64,
    pub role: String,
    pub message_kind: String,
    pub preview_text: Option<String>,
    pub collapsed_by_default: bool,
    pub chunks_json: String,
    pub timestamp: String,
}

#[derive(Clone, Serialize)]
pub struct ApiLiveTurn {
    pub chunks_json: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Clone, Serialize)]
pub struct ApiSession {
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ApiScopeActivity {
    pub session: Option<ApiSession>,
    pub live_turn: Option<ApiLiveTurn>,
    pub has_active_turn: bool,
}

#[derive(Clone, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_task_id: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ApiPlanProgress {
    pub completed: usize,
    pub total: usize,
}

#[derive(Clone, Serialize)]
pub struct ApiThreadSummary {
    pub thread: ApiThread,
    pub activity: ApiScopeActivity,
    pub plan_progress: Option<ApiPlanProgress>,
}

#[derive(Clone, Serialize)]
pub struct ApiThreadDetail {
    pub thread: ApiThread,
    pub activity: ApiScopeActivity,
    pub plan: Option<PlanSnapshot>,
}

#[derive(Serialize)]
pub struct ApiWorkspaceSnapshot {
    pub project: ApiProject,
    pub project_activity: ApiScopeActivity,
    pub project_history: Vec<ApiChatMessage>,
    pub surface: crate::backend::server::web_routes::projects::ApiProjectSurface,
    pub threads: Vec<ApiThreadSummary>,
    pub thread_detail: Option<ApiThreadDetail>,
    pub thread_history: Vec<ApiChatMessage>,
    pub focused_task: Option<crate::backend::tasks::Task>,
    pub tasks: Vec<crate::backend::tasks::Task>,
    pub librarian_activity: ApiScopeActivity,
    pub librarian_history: Vec<ApiChatMessage>,
}

pub fn to_api_project(project: &Project) -> ApiProject {
    ApiProject {
        id: project.id,
        name: project.name.clone(),
        created_at: project.created_at.clone(),
        workspaces: project.workspaces.clone(),
        shepherd_cwd: project.shepherd_cwd.clone(),
    }
}

pub fn to_api_message(message: &ShepherdChatMessage) -> ApiChatMessage {
    ApiChatMessage {
        id: message.id,
        role: message.role.clone(),
        message_kind: message.message_kind.clone(),
        preview_text: message.preview_text.clone(),
        collapsed_by_default: message.collapsed_by_default,
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
        highlight: thread.highlight.clone(),
        focused_task_id: thread.focused_task_id.clone(),
    }
}

pub fn extract_latest_plan(messages: &[ShepherdChatMessage]) -> Option<PlanSnapshot> {
    plans::extract_latest_plan(messages)
}

pub fn plan_progress_from_messages(messages: &[ShepherdChatMessage]) -> Option<ApiPlanProgress> {
    let plan = extract_latest_plan(messages)?;
    let total = plan.plan.len();
    let completed = plan
        .plan
        .iter()
        .filter(|step| step.status == "completed")
        .count();
    Some(ApiPlanProgress { completed, total })
}
