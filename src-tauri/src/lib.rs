//! Hirsel - Herd your AI coding agents
//!
//! This library provides the core functionality for hirsel,
//! including file system utilities, state management, CLI
//! command routing, and the Tauri GUI integration.

pub mod cli;
pub mod core;
pub mod gui;
pub mod worker;

// Re-export commonly used types
pub use cli::{parse_cli, parse_worker_cli, run_cli, Cli, Commands, WorkerCli, WorkerCommands};
pub use core::Files;
pub use core::state;
pub use worker::{WorkerRunner, WorkerConfig, WorkerError};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(gui::get_handlers())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
