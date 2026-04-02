mod api;
mod support;

use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;

use super::AppState;

pub fn build_web_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(api::health))
        .route("/api/connect", post(api::connect))
        .route("/api/logout", post(api::logout))
        .route("/api/projects", get(api::list_projects))
        .route("/api/projects/probe", get(api::probe_project_create_api))
        .route("/api/projects", post(api::create_project_api))
        .route(
            "/api/projects/{project_id}/preparation",
            get(api::get_project_preparation),
        )
        .route(
            "/api/projects/{project_id}/preparation/retry",
            post(api::retry_project_preparation),
        )
        .route(
            "/api/projects/{project_id}/page",
            get(api::get_project_page),
        )
        .route(
            "/api/projects/{project_id}/workspace/file",
            get(api::get_workspace_file),
        )
        .route("/api/projects/{project_id}", delete(api::delete_project))
        .route(
            "/api/projects/{project_id}/settings",
            post(api::save_project_settings),
        )
        .route(
            "/api/projects/{project_id}/chat/send",
            post(api::send_chat_message),
        )
        .route("/api/projects/{project_id}/chat/stop", post(api::stop_chat))
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/page",
            get(api::get_thread_page),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/chat/send",
            post(api::send_thread_message),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/chat/stop",
            post(api::stop_thread_chat),
        )
        .route("/api/settings", get(api::get_settings))
        .route("/api/settings/provider", post(api::save_llm_provider))
        .route(
            "/api/settings/provider/codex/device/start",
            post(api::start_codex_device_flow),
        )
        .route(
            "/api/settings/provider/codex/device/poll",
            post(api::poll_codex_device_flow),
        )
        .route("/api/settings/openrouter", post(api::save_openrouter_key))
        .route("/api/settings/github", post(api::save_github_token))
        .route("/api/settings/tavily", post(api::save_tavily_key))
}
