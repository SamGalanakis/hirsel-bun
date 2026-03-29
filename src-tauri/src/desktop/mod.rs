//! Thin desktop-host module for Tauri.
//!
//! This namespace only contains the small IPC/config bridge the native shell
//! needs before opening the backend-served UI.

pub mod commands;
pub mod error;

pub use commands::*;
pub use error::{ErrorCode, GuiError, GuiResult};
