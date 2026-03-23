//! Tauri IPC commands for the Hirsel GUI
//!
//! This module provides the bridge between the frontend (SolidJS/TypeScript)
//! and the backend (Rust). All commands are exposed via Tauri's IPC system.
//!
//! Types are designed to match the TypeScript definitions in src/lib/types.ts.

mod concerns;
mod config_cmd;
mod credentials;
mod debug;
mod delivery;
mod events;
mod files;
mod filesystem;
mod projects;
mod routes;
mod shepherd;
pub mod types;
mod workers;
mod worktree;

// Re-export types for use by other modules
pub use types::*;

// Re-export the event stream manager for state management
pub use events::WorkerEventStreamManager;

/// Extension trait for converting Result errors to String
///
/// Provides a concise alternative to `.map_err(|e| e.to_string())` for Tauri commands.
pub trait ResultExt<T, E: ToString> {
    fn str_err(self) -> Result<T, String>;
    fn context(self, msg: &str) -> Result<T, String>;
}

impl<T, E: ToString> ResultExt<T, E> for Result<T, E> {
    fn str_err(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
    fn context(self, msg: &str) -> Result<T, String> {
        self.map_err(|e| format!("{}: {}", msg, e.to_string()))
    }
}

/// Generate the Tauri invoke handler with all commands
pub fn get_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // Asset commands (project-level)
        files::save_project_asset,
        files::get_project_assets_path,
        files::open_project_assets_folder,
        // Route worker commands
        workers::get_route_workers,
        workers::open_route_worker_terminal,
        workers::restart_route_worker,
        // Route worker events commands (streaming)
        events::get_route_worker_events,
        events::clear_route_worker_events,
        events::start_route_worker_event_stream,
        events::stop_route_worker_event_stream,
        // Concern / notification commands
        concerns::get_worker_concerns,
        concerns::mark_worker_concern_read,
        concerns::mark_route_concerns_read,
        concerns::resolve_worker_concern,
        concerns::get_all_unread_notifications,
        // Config commands
        config_cmd::get_config,
        config_cmd::get_config_defaults,
        config_cmd::save_config,
        config_cmd::check_backend_health,
        config_cmd::codex_device_start_gui,
        config_cmd::codex_device_poll_gui,
        config_cmd::codex_device_exchange_gui,
        // Credential commands
        credentials::store_credential,
        credentials::delete_credential,
        credentials::has_credential,
        credentials::get_credential,
        credentials::get_credential_masked,
        // Frontend logging (dev mode)
        debug::log_frontend,
        debug::log_frontend_batch,
        // Debug commands
        debug::get_version,
        debug::get_process_counts,
        debug::kill_orphaned_worker_processes,
        debug::get_profiling_enabled,
        debug::save_profiling_data,
        debug::get_process_memory,
        // Filesystem commands
        filesystem::pick_folder,
        filesystem::suggest_paths,
        filesystem::validate_repo,
        // Project management commands
        projects::list_projects,
        projects::get_project,
        projects::get_project_focus_view,
        projects::get_project_retained_context,
        projects::get_project_surface,
        projects::create_project,
        projects::create_project_from_path,
        projects::update_project,
        projects::update_project_focus_view,
        projects::update_project_retained_context,
        projects::update_project_name,
        projects::delete_project,
        // Delivery commands (run-based)
        delivery::get_delivery_state,
        delivery::check_merge_state,
        delivery::get_conflicting_files,
        delivery::check_staleness,
        delivery::push_run_branch,
        delivery::create_run_pr,
        delivery::auto_merge_run,
        delivery::generate_pr_title,
        delivery::generate_pr_body,
        delivery::get_delivery_branch_name,
        // Delivery validation
        delivery::validate_delivery_target,
        // Board delivery commands
        delivery::get_board_versions,
        delivery::get_latest_board_version,
        delivery::get_current_board_delivery,
        delivery::start_board_delivery,
        delivery::retry_board_delivery,
        delivery::get_delivery_attempts,
        delivery::complete_board_delivery,
        delivery::abandon_board_delivery,
        // Route work-tree commands
        worktree::get_route_work_tree,
        worktree::create_work_item,
        worktree::reparent_work_item,
        worktree::split_work_item,
        worktree::assign_work_item,
        worktree::reopen_work_item,
        worktree::archive_work_item,
        // Shepherd chat commands
        shepherd::commands::start_shepherd_session,
        shepherd::commands::send_shepherd_message,
        shepherd::commands::stop_shepherd_session,
        shepherd::commands::list_shepherd_sessions,
        shepherd::commands::get_shepherd_history,
        shepherd::commands::clear_shepherd_history,
        shepherd::commands::save_shepherd_message,
        // Route commands
        routes::list_routes,
        routes::list_archived_routes,
        routes::get_route,
        routes::get_route_by_name,
        routes::get_route_tree,
        routes::list_route_repos,
        routes::create_route_repo,
        routes::update_route_repo,
        routes::delete_route_repo,
        routes::set_default_route_repo,
        routes::create_route,
        routes::archive_route,
        routes::set_active_route,
        routes::get_active_route,
    ]
}
