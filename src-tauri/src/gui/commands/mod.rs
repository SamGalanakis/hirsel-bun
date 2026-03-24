//! Tauri IPC commands for the thin desktop shell.
//!
//! The desktop wrapper only persists backend connection details and performs
//! basic backend reachability checks before handing off to the backend-served UI.

pub(crate) mod concerns;
mod config_cmd;
pub(crate) mod delivery;
pub(crate) mod events;
pub(crate) mod projects;
pub(crate) mod routes;
pub(crate) mod shepherd;
pub mod types;
pub(crate) mod workers;
pub(crate) mod worktree;

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
        config_cmd::check_backend_health,
        // Stateful product operations intentionally stay on the backend over HTTP/SSE.
    ]
}
