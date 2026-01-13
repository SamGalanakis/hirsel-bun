//! Hirsel - Herd your AI coding agents
//!
//! This library provides the core functionality for hirsel,
//! including file system utilities, state management, and
//! the Tauri GUI integration.

pub mod core;

// Re-export core types for convenience
pub use core::Files;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
