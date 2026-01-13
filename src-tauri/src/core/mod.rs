//! Core modules for hirsel functionality.
//!
//! This module provides the shared infrastructure used by both
//! the CLI and GUI components of hirsel.

pub mod files;

// Re-export commonly used types
pub use files::Files;
