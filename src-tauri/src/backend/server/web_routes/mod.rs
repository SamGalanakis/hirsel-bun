mod api;
mod connect;
mod projects;
mod settings;
mod support;
mod threads;

use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;

use super::AppState;

pub fn build_web_routes() -> Router<Arc<AppState>> {
    Router::new()
        // JSON API routes (consumed by SolidJS frontend)
        .route("/api/projects", get(api::list_projects))
        .route(
            "/api/projects/{project_id}/page",
            get(api::get_project_page),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/page",
            get(api::get_thread_page),
        )
        .route(
            "/api/projects/{project_id}/chat/send",
            post(api::send_chat_message),
        )
        .route("/api/projects/{project_id}/chat/stop", post(api::stop_chat))
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/chat/send",
            post(api::send_thread_message),
        )
        .route(
            "/api/projects/{project_id}/threads/{thread_id}/chat/stop",
            post(api::stop_thread_chat),
        )
        .route("/api/connect", post(api::connect))
        .route("/api/projects/{project_id}", delete(api::delete_project))
        // Legacy HTML routes (kept until SPA fully replaces them)
        .route("/health", get(connect::health))
        .route("/", get(connect::app_root))
        .route("/connect", get(connect::connect_page))
        .route("/connect/session", post(connect::connect_session))
        .route("/connect/bootstrap", get(connect::connect_bootstrap))
        .route("/connect/logout", post(connect::connect_logout))
        .route("/static/webui.css", get(connect::webui_css))
        .route("/static/datastar.js", get(connect::datastar_bundle))
        .route("/app", get(projects::app_home))
        .route("/app/new", get(projects::new_project_page))
        .route("/app/projects/setup", post(projects::setup_project))
        .route("/app/projects", post(projects::create_project))
        .route(
            "/app/projects/confirm-create",
            post(projects::confirm_create_project),
        )
        .route("/app/settings", get(settings::settings_page))
        .route("/app/settings/llm", post(settings::save_llm_settings))
        .route(
            "/app/settings/openrouter",
            post(settings::save_openrouter_key),
        )
        .route("/app/settings/tavily", post(settings::save_tavily_key))
        .route(
            "/app/settings/codex/start",
            post(settings::start_codex_device),
        )
        .route(
            "/app/settings/codex/stream",
            get(settings::stream_codex_device),
        )
        .route(
            "/app/projects/{project_id}/prepare",
            get(projects::project_preparation_page),
        )
        .route(
            "/app/projects/{project_id}/prepare/stream",
            get(projects::project_preparation_stream),
        )
        .route(
            "/app/projects/{project_id}/prepare/retry",
            post(projects::retry_project_preparation),
        )
        .route("/app/projects/{project_id}", get(projects::project_page))
        .route(
            "/app/projects/{project_id}/focus",
            get(projects::project_focus_page),
        )
        .route(
            "/app/projects/{project_id}/stream",
            get(projects::project_stream),
        )
        .route(
            "/app/projects/{project_id}/chat/send",
            post(projects::send_chat_message),
        )
        .route(
            "/app/projects/{project_id}/chat/stop",
            post(projects::stop_chat),
        )
        .route(
            "/app/projects/{project_id}/threads/{thread_id}",
            get(threads::thread_detail_page),
        )
        .route(
            "/app/projects/{project_id}/threads/{thread_id}/stream",
            get(threads::thread_detail_stream),
        )
        .route(
            "/app/projects/{project_id}/threads/{thread_id}/chat/send",
            post(threads::send_thread_message),
        )
        .route(
            "/app/projects/{project_id}/threads/{thread_id}/chat/stop",
            post(threads::stop_thread_chat),
        )
        .route(
            "/app/projects/{project_id}/settings",
            get(projects::project_settings_page),
        )
        .route(
            "/app/projects/{project_id}/settings/project",
            post(projects::save_project_settings),
        )
        .route(
            "/app/projects/{project_id}/delete",
            post(projects::delete_project),
        )
}
