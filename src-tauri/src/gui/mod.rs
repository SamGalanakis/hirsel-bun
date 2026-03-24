//! GUI module for Tauri IPC commands
//!
//! This module provides the tiny bridge between the desktop wrapper and the
//! backend. The product UI itself is served by the backend over HTTP/SSE.

pub mod commands;
pub mod error;

pub use commands::*;
pub use error::{ErrorCode, GuiError, GuiResult};
