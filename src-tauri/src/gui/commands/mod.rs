//! Tauri IPC commands for the Hirsel GUI
//!
//! This module provides the bridge between the frontend (Alpine.js/TypeScript)
//! and the backend (Rust). All commands are exposed via Tauri's IPC system.
//!
//! Types are designed to match the TypeScript definitions in src/lib/types.ts.

mod config_cmd;
mod credentials;
mod debug;
mod delivery;
mod delta;
mod drafts;
mod events;
mod files;
mod filesystem;
mod ide;
mod logs;
mod messages;
mod project_messages;
mod projects;
mod routes;
mod runs;
mod shepherd;
pub mod types;
mod workers;

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

/// Helper to get SQLiteState for a run, with standard error handling
pub async fn get_run_state(run_name: &str) -> Result<crate::core::state::SQLiteState, String> {
    let db_path = crate::core::config::run_dir(run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }
    crate::core::state::SQLiteState::new(run_name)
        .await
        .context("Failed to open database")
}

/// Helper to get the work directory for a run, with validation
pub fn get_run_work_dir(run_name: &str) -> Result<std::path::PathBuf, String> {
    let run_path = crate::core::hirsel_dir().join("runs").join(run_name);
    if !run_path.exists() {
        return Err(format!("Run not found: {}", run_name));
    }
    let work_dir = run_path.join("work");
    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }
    Ok(work_dir)
}

/// Generate the Tauri invoke handler with all commands
pub fn get_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // Run commands
        runs::get_runs,
        runs::get_runs_if_changed,
        runs::get_run_detail,
        runs::pause_run,
        runs::resume_run,
        runs::delete_run,
        runs::delete_all_runs,
        runs::deliver_run,
        // Draft commands
        drafts::validate_repo,
        drafts::create_draft,
        drafts::clone_run,
        drafts::update_draft,
        drafts::start_draft,
        drafts::change_starting_point,
        // Spec/Eval file commands
        files::read_spec_file,
        files::write_spec_file,
        files::read_eval_file,
        files::write_eval_file,
        // Asset commands (run-level)
        files::save_asset,
        files::import_asset_from_path,
        files::open_assets_folder,
        files::get_assets_path,
        // Asset commands (project-level)
        files::save_project_asset,
        files::get_project_assets_path,
        files::open_project_assets_folder,
        // Worker commands
        workers::get_workers,
        workers::attach_worker,
        workers::open_worker_terminal,
        workers::detach_worker,
        workers::restart_worker,
        // Eval log commands
        logs::get_eval_log,
        logs::get_eval_log_by_path,
        // History commands
        logs::get_history,
        // Eval commands
        logs::get_eval_spec,
        logs::get_evals,
        // Worker events commands (streaming)
        events::get_worker_events,
        events::clear_worker_events,
        events::start_worker_event_stream,
        events::stop_worker_event_stream,
        // Message commands
        messages::get_all_unread_notifications,
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
        debug::get_daemon_health,
        debug::ensure_daemon_running,
        debug::get_profiling_enabled,
        debug::save_profiling_data,
        debug::get_process_memory,
        // Filesystem commands
        filesystem::pick_folder,
        filesystem::suggest_paths,
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
        // Board tree commands (unified board with spec/task/eval nodes)
        delta::get_board_tree,
        delta::create_board_node,
        delta::update_board_node,
        delta::delete_board_node,
        delta::move_board_node,
        delta::reset_project_tree,
        delta::start_shepherd_run,
        delta::get_project_run,
        delta::complete_board_node,
        delta::sync_shepherd_changes,
        delta::sync_and_get_shepherd_view,
        delta::sync_and_get_shepherd_view_if_changed,
        // Shepherd chat commands
        shepherd::commands::start_shepherd_session,
        shepherd::commands::send_shepherd_message,
        shepherd::commands::stop_shepherd_session,
        shepherd::commands::list_shepherd_sessions,
        shepherd::commands::get_shepherd_history,
        shepherd::commands::clear_shepherd_history,
        shepherd::commands::save_shepherd_message,
        // Project Messages (Sheepfold) commands
        project_messages::get_project_messages,
        project_messages::get_project_threads,
        project_messages::send_project_message,
        project_messages::mark_project_messages_read,
        project_messages::get_project_unread_count,
        // IDE commands
        ide::open_in_ide,
        // Route commands
        routes::list_routes,
        routes::get_route,
        routes::get_route_by_name,
        routes::get_route_tree,
        routes::list_route_repos,
        routes::create_route_repo,
        routes::update_route_repo,
        routes::delete_route_repo,
        routes::set_default_route_repo,
        routes::create_route,
        routes::delete_route,
        routes::set_active_route,
        routes::get_active_route,
    ]
}
