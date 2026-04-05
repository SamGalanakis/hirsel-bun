//! Tauri IPC commands for the thin desktop shell.
//!
//! The desktop wrapper only persists backend connection details before handing
//! off to the backend-served UI.

mod config_cmd;
pub mod types;

// Re-export types for use by other modules
pub use types::*;

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
        config_cmd::get_config,
        config_cmd::save_config,
        config_cmd::open_backend_window,
        // Stateful product operations intentionally stay on the backend over HTTP/SSE.
    ]
}
