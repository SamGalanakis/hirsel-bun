//! GUI module for Tauri IPC commands
//!
//! This module provides the bridge between the frontend (Alpine.js/TypeScript)
//! and the backend (Rust). All commands are exposed via Tauri's IPC system.

pub mod commands;
pub mod error;

pub use commands::*;
pub use error::{ErrorCode, GuiError, GuiResult};
