mod auth;
mod common;
mod projects;
mod settings;
mod skills;
mod tasks;
mod terminal;
mod threads;
mod workspace_browser;

use std::sync::Arc;

use axum::routing::{get, patch, post};
use axum::Router;

use super::AppState;

pub fn build_web_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(auth::health))
        .route("/api/connect", post(auth::connect))
        .route("/api/logout", post(auth::logout))
        .route(
            "/api/projects",
            get(projects::list_projects).post(projects::create_project_api),
        )
        .route(
            "/api/projects/{project_id}",
            get(projects::get_project).delete(projects::delete_project),
        )
        .route(
            "/api/projects/{project_id}/events",
            get(projects::project_events),
        )
        .route(
            "/api/projects/{project_id}/activity",
            get(projects::get_project_activity),
        )
        .route(
            "/api/projects/{project_id}/history",
            get(projects::get_project_history),
        )
        .route(
            "/api/projects/{project_id}/surface",
            get(projects::get_project_surface),
        )
        .route(
            "/api/projects/{project_id}/workspace-snapshot",
            get(projects::get_workspace_snapshot),
        )
        .route(
            "/api/projects/{project_id}/skills",
            get(skills::list_project_skills),
        )
        .route(
            "/api/projects/{project_id}/terminal",
            get(terminal::project_terminal),
        )
        .route(
            "/api/projects/{project_id}/workspace/roots",
            get(workspace_browser::list_workspace_roots),
        )
        .route(
            "/api/projects/{project_id}/workspace/tree",
            get(workspace_browser::list_workspace_tree),
        )
        .route(
            "/api/projects/{project_id}/workspace/complete",
            get(workspace_browser::complete_workspace_path_entries),
        )
        .route(
            "/api/projects/{project_id}/workspace/file",
            get(workspace_browser::get_workspace_file).put(workspace_browser::save_workspace_file),
        )
        .route(
            "/api/projects/{project_id}/workspace/upload",
            post(workspace_browser::upload_workspace_files),
        )
        .route(
            "/api/projects/{project_id}/workspace/download",
            get(workspace_browser::download_workspace_file),
        )
        .route(
            "/api/projects/{project_id}/workspace/diff",
            get(workspace_browser::get_workspace_diff),
        )
        .route(
            "/api/projects/{project_id}/workspace/diff/file",
            get(workspace_browser::get_workspace_diff_file),
        )
        .route(
            "/api/projects/{project_id}/workspace/search",
            get(workspace_browser::search_workspace),
        )
        .route(
            "/api/projects/{project_id}/knowledge-graph",
            get(projects::get_knowledge_graph),
        )
        .route(
            "/api/projects/{project_id}/librarian/activity",
            get(projects::get_librarian_activity),
        )
        .route(
            "/api/projects/{project_id}/librarian/history",
            get(projects::get_librarian_history),
        )
        .route(
            "/api/projects/{project_id}/librarian/chat/send",
            post(projects::send_librarian_message),
        )
        .route(
            "/api/projects/{project_id}/librarian/chat/stop",
            post(projects::stop_librarian_chat),
        )
        .route(
            "/api/projects/{project_id}/settings",
            post(projects::save_project_settings),
        )
        .route(
            "/api/projects/{project_id}/chat/send",
            post(projects::send_project_chat_message),
        )
        .route(
            "/api/projects/{project_id}/chat/stop",
            post(projects::stop_project_chat),
        )
        .route(
            "/api/projects/{project_id}/threads",
            get(threads::list_threads),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}",
            get(threads::get_thread),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/history",
            get(threads::get_thread_history),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/chat/send",
            post(threads::send_thread_message),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/chat/stop",
            post(threads::stop_thread_chat),
        )
        // Tasks
        .route(
            "/api/projects/{project_id}/tasks",
            get(tasks::list_tasks).post(tasks::create_task),
        )
        .route(
            "/api/projects/{project_id}/tasks/{task_id}",
            patch(tasks::update_task).delete(tasks::delete_task),
        )
        .route(
            "/api/projects/{project_id}/tasks/reorder",
            post(tasks::reorder_tasks),
        )
        .route("/api/settings", get(settings::get_settings))
        .route("/api/settings/provider", post(settings::save_llm_provider))
        .route("/api/settings/models", post(settings::save_role_models))
        .route(
            "/api/settings/provider/codex/device/start",
            post(settings::start_codex_device_flow),
        )
        .route(
            "/api/settings/provider/codex/device/poll",
            post(settings::poll_codex_device_flow),
        )
        .route(
            "/api/settings/openrouter",
            post(settings::save_openrouter_key),
        )
        .route("/api/settings/github", post(settings::save_github_token))
        .route("/api/settings/tavily", post(settings::save_tavily_key))
}
