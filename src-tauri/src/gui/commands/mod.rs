//! Tauri IPC commands for the Hirsel GUI
//!
//! This module provides the bridge between the frontend (Alpine.js/TypeScript)
//! and the backend (Rust). All commands are exposed via Tauri's IPC system.
//!
//! Types are designed to match the TypeScript definitions in src/lib/types.ts.

mod chat;
mod config_cmd;
mod credentials;
mod debug;
mod delivery;
mod delta;
mod docs;
mod drafts;
mod events;
mod files;
mod filesystem;
mod gyp;
mod logs;
mod messages;
mod project_messages;
mod projects;
mod runs;
mod tasks;
pub mod types;
mod workers;

// Re-export types for use by other modules
pub use types::*;

// Re-export the event stream manager for state management
pub use events::WorkerEventStreamManager;

/// Helper to convert any error to String for Tauri command results
pub fn err_string<E: ToString>(e: E) -> String {
    e.to_string()
}

/// Helper to get SQLiteState for a run, with standard error handling
pub fn get_run_state(run_name: &str) -> Result<crate::core::state::SQLiteState, String> {
    let db_path = crate::core::config::run_dir(run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }
    crate::core::state::SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))
}

// Re-export the chat orchestrator manager for state management
pub use chat::ChatOrchestratorManager;

// GypChatStore is available from crate::core::gyp_chat for modules that need it

/// Generate the Tauri invoke handler with all commands
pub fn get_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // Run commands
        runs::get_runs,
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
        // Task commands
        tasks::get_tasks,
        tasks::add_task,
        tasks::delete_task,
        tasks::complete_task,
        tasks::unclaim_task,
        tasks::reopen_task,
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
        // Worker events commands (ACP-based streaming)
        events::get_worker_events,
        events::clear_worker_events,
        events::start_worker_event_stream,
        events::stop_worker_event_stream,
        // Message commands
        messages::get_messages,
        messages::get_threads,
        messages::get_all_unread_notifications,
        messages::send_message,
        messages::mark_messages_read,
        // Config commands
        config_cmd::get_config,
        config_cmd::get_config_defaults,
        config_cmd::save_config,
        config_cmd::get_tailscale_info,
        config_cmd::check_ssh_runner,
        // Credential commands
        credentials::store_credential,
        credentials::delete_credential,
        credentials::has_credential,
        credentials::get_credential,
        credentials::get_credential_masked,
        // Chat session commands
        chat::start_chat_session,
        chat::send_chat_message,
        chat::respond_chat_permission,
        chat::stop_chat_session,
        chat::list_chat_sessions,
        // Frontend logging (dev mode)
        debug::log_frontend,
        // Debug commands
        debug::get_version,
        debug::get_process_counts,
        debug::kill_orphaned_acp_processes,
        // Filesystem commands
        filesystem::pick_folder,
        filesystem::suggest_paths,
        // Project management commands
        projects::list_projects,
        projects::get_project,
        projects::create_project,
        projects::create_project_from_path,
        projects::update_project,
        projects::update_project_name,
        projects::delete_project,
        // Unified Gyp commands
        gyp::start_gyp_session,
        gyp::send_gyp_message,
        gyp::save_gyp_message,
        gyp::get_gyp_history,
        gyp::clear_gyp_history,
        gyp::stop_gyp_session,
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
        delivery::delivery_branch_name,
        // Board delivery commands (delta dispatch system)
        delivery::get_board_versions,
        delivery::get_latest_board_version,
        delivery::get_current_board_delivery,
        delivery::start_board_delivery,
        delivery::get_board_delivery_status,
        delivery::retry_board_delivery,
        delivery::get_delivery_attempts,
        delivery::complete_board_delivery,
        delivery::abandon_board_delivery,
        // Delta dispatch commands (unified board with draft/live trees)
        delta::get_draft_tree,
        delta::get_live_tree,
        delta::create_draft_node,
        delta::update_draft_node,
        delta::delete_draft_node,
        delta::move_draft_node,
        delta::reset_project_tree,
        delta::compute_tree_diff,
        delta::get_diff_summary,
        delta::dispatch_deltas,
        delta::preview_delta_dispatch,
        delta::get_project_run,
        delta::complete_live_node,
        delta::complete_revert,
        delta::get_dual_trees,
        delta::sync_gyp_changes,
        // Docs commands
        docs::get_project_docs,
        // Project Messages (Sheepfold) commands
        project_messages::get_project_messages,
        project_messages::get_project_threads,
        project_messages::send_project_message,
        project_messages::mark_project_messages_read,
        project_messages::get_project_unread_count,
    ]
}
