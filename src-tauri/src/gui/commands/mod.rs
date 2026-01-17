//! Tauri IPC commands for the Hirsel GUI
//!
//! This module provides the bridge between the frontend (Alpine.js/TypeScript)
//! and the backend (Rust). All commands are exposed via Tauri's IPC system.
//!
//! Types are designed to match the TypeScript definitions in src/lib/types.ts.

mod chat;
mod config_cmd;
mod debug;
mod drafts;
mod events;
mod files;
pub mod helpers;
mod logs;
mod messages;
mod runs;
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
        runs::deliver_run,
        // Draft commands
        drafts::validate_repo,
        drafts::init_project_repo,
        drafts::create_draft,
        drafts::clone_run,
        drafts::update_draft,
        drafts::start_draft,
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
        // Chat session commands
        chat::start_chat_session,
        chat::send_chat_message,
        chat::respond_chat_permission,
        chat::stop_chat_session,
        chat::list_chat_sessions,
        // Frontend logging (dev mode)
        debug::log_frontend,
        // Debug commands
        debug::get_process_counts,
        debug::kill_orphaned_acp_processes,
        // Gyp chat history commands
        debug::get_gyp_chat_history,
        debug::save_gyp_message,
        debug::clear_gyp_chat_history,
    ]
}
