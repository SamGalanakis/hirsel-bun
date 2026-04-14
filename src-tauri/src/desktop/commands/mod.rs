//! Tauri IPC commands for the thin desktop shell.
//!
//! The desktop shell currently boots directly into the backend-served UI and
//! does not expose product IPC commands of its own.

/// Generate the Tauri invoke handler with all commands
pub fn get_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![]
}
