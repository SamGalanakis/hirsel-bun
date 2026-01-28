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
mod dispatch;
mod drafts;
mod events;
mod files;
mod filesystem;
mod gyp;
mod logs;
mod messages;
mod projects;
mod runs;
mod specflow;
mod tasks;
pub mod types;
mod workers;

// Re-export types for use by other modules
pub use types::*;

// Re-export the event stream manager for state management
pub use events::WorkerEventStreamManager;

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
        // Asset commands
        files::save_asset,
        files::import_asset_from_path,
        files::open_assets_folder,
        files::get_assets_path,
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
        // SpecFlow board task commands
        specflow::get_board_tasks,
        specflow::get_board_task_tree,
        specflow::create_board_task,
        specflow::update_board_task,
        specflow::delete_board_task,
        specflow::move_board_task,
        // SpecFlow board eval commands
        specflow::get_board_evals,
        specflow::create_board_eval,
        specflow::update_board_eval,
        specflow::delete_board_eval,
        // Board bookmark commands
        specflow::get_bookmarks,
        specflow::save_bookmark,
        specflow::delete_bookmark,
        // Board sync commands
        specflow::export_board_for_agent,
        specflow::import_board_from_agent,
        specflow::get_board_directory,
        specflow::poll_board_changes,
        // Unified Gyp commands
        gyp::start_gyp_session,
        gyp::send_gyp_message,
        gyp::save_gyp_message,
        gyp::get_gyp_history,
        gyp::clear_gyp_history,
        gyp::stop_gyp_session,
        // Dispatch commands
        dispatch::preview_dispatch,
        dispatch::prepare_dispatch,
        dispatch::prepare_multi_dispatch,
        dispatch::dispatch_board_run,
        dispatch::get_multi_dispatch_scope,
        dispatch::record_dispatch,
        dispatch::get_task_runs,
        dispatch::get_all_task_runs,
        dispatch::create_dispatch_snapshot,
        // Delivery commands
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
    ]
}
