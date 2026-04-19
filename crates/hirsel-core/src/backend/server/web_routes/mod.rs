mod auth;
mod canvas;
mod comments;
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
            "/api/projects/{project_id}/settings",
            post(projects::save_project_settings),
        )
        .route(
            "/api/projects/{project_id}/focus",
            post(projects::record_project_focus),
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
        .route(
            "/api/projects/{project_id}/spawn-thread",
            post(threads::spawn_thread),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/inspect",
            get(threads::inspect_thread),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/merge",
            post(threads::merge_thread),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/merge/retry",
            post(threads::merge_thread_retry),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/discard",
            post(threads::discard_thread),
        )
        // Comments
        .route(
            "/api/projects/{project_id}/nodes/{kind}/{node_id}/comments",
            get(comments::list_comments).post(comments::create_comment),
        )
        .route(
            "/api/projects/{project_id}/comments/{comment_id}/resolve",
            post(comments::resolve_comment),
        )
        // Staleness: user-initiated verify-node trigger (T1)
        .route(
            "/api/projects/{project_id}/nodes/{kind}/{node_id}/verify",
            post(projects::request_node_verify),
        )
        // Staleness: on-demand ambient sweep (A1–A4) for this project
        .route(
            "/api/projects/{project_id}/staleness/sweep",
            post(projects::run_staleness_sweep),
        )
        // Background jobs: inspector listing recent librarian jobs
        .route(
            "/api/projects/{project_id}/librarian-jobs",
            get(projects::list_librarian_jobs),
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
        .route(
            "/api/projects/{project_id}/tasks/{task_id}/dispatch",
            post(tasks::dispatch_task),
        )
        .route(
            "/api/projects/{project_id}/tasks/{task_id}/review",
            post(tasks::review_action),
        )
        // Canvas
        .route("/api/projects/{project_id}/canvas", get(canvas::get_canvas))
        .route(
            "/api/projects/{project_id}/canvas/layout",
            patch(canvas::patch_layout).delete(canvas::delete_layout),
        )
        .route(
            "/api/projects/{project_id}/canvas/node",
            post(canvas::create_canvas_node),
        )
        .route(
            "/api/projects/{project_id}/canvas/node/{kind}/{node_id}",
            patch(canvas::update_canvas_node).delete(canvas::delete_canvas_node),
        )
        .route(
            "/api/projects/{project_id}/companion/actions",
            get(canvas::drain_companion_actions),
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
