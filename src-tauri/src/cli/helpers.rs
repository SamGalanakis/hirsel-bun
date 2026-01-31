//! CLI helper utilities for orchestrator integration.
//!
//! This module provides helpers for CLI commands to use the Orchestrator trait,
//! enabling both local and remote operation through the same code paths.

use crate::core::orchestrator::{create_orchestrator, Orchestrator, OrchestratorError};
use serde::Serialize;
use std::future::Future;

/// Create a tokio runtime and block on a future.
///
/// This helper allows CLI commands (which run synchronously) to call
/// async orchestrator methods.
///
/// # Example
///
/// ```ignore
/// use crate::cli::helpers::{block_on, get_orchestrator};
///
/// fn list_runs(profile: Option<&str>) -> Result<(), anyhow::Error> {
///     let orch = get_orchestrator(profile)?;
///     let runs = block_on(orch.list_runs())?;
///     for run in runs {
///         println!("{}: {}", run.name, run.status);
///     }
///     Ok(())
/// }
/// ```
pub fn block_on<F: Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime")
        .block_on(f)
}

/// Get an orchestrator instance for the given profile.
///
/// If profile is None, uses the default profile from config.
/// Returns a boxed Orchestrator trait object that can be either
/// LocalOrchestrator or RemoteOrchestrator.
pub fn get_orchestrator(profile: Option<&str>) -> Result<Box<dyn Orchestrator>, OrchestratorError> {
    create_orchestrator(profile)
}

/// Helper to run an async operation and convert the result to anyhow::Error
pub fn run_async<T, F>(profile: Option<&str>, op: F) -> anyhow::Result<T>
where
    F: FnOnce(
        &dyn Orchestrator,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<T, OrchestratorError>> + '_>>,
{
    let orch = get_orchestrator(profile)?;
    let result = block_on(op(orch.as_ref()))?;
    Ok(result)
}

// =============================================================================
// CLI Output Helper
// =============================================================================

/// Helper for consistent CLI output formatting.
///
/// Handles both human-readable and JSON output formats, ensuring
/// consistent structure across all CLI commands.
///
/// # Example
///
/// ```ignore
/// use crate::cli::helpers::CliOutput;
///
/// fn pause_run(name: &str, json: bool) -> Result<(), Error> {
///     let output = CliOutput::new(json);
///
///     match do_pause(name) {
///         Ok(()) => output.success(&format!("Paused run '{}'", name)),
///         Err(e) => output.error(&e.to_string()),
///     }
///     Ok(())
/// }
/// ```
pub struct CliOutput {
    json: bool,
}

impl CliOutput {
    /// Create a new CLI output helper.
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    /// Output a success message.
    ///
    /// In JSON mode, outputs: `{"success": true, "message": "..."}`
    /// In text mode, prints the message to stdout.
    pub fn success(&self, message: &str) {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": true,
                    "message": message
                }))
                .unwrap()
            );
        } else {
            println!("{}", message);
        }
    }

    /// Output a success message with additional data.
    ///
    /// In JSON mode, outputs: `{"success": true, "message": "...", ...data}`
    /// In text mode, prints only the message to stdout.
    pub fn success_with_data<T: Serialize>(&self, message: &str, data: T) {
        if self.json {
            // Merge message into data
            let mut value = serde_json::to_value(&data).unwrap_or(serde_json::json!({}));
            if let Some(obj) = value.as_object_mut() {
                obj.insert("success".to_string(), serde_json::json!(true));
                obj.insert("message".to_string(), serde_json::json!(message));
            }
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
        } else {
            println!("{}", message);
        }
    }

    /// Output an error message and exit.
    ///
    /// In JSON mode, outputs: `{"success": false, "error": "..."}`
    /// In text mode, prints to stderr with "Error: " prefix.
    pub fn error(&self, error: &str) -> ! {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": false,
                    "error": error
                }))
                .unwrap()
            );
            std::process::exit(1);
        } else {
            eprintln!("Error: {}", error);
            std::process::exit(1);
        }
    }

    /// Output a "not found" error and exit.
    ///
    /// Convenience method for common "X not found" errors.
    pub fn not_found(&self, resource: &str, name: &str) -> ! {
        self.error(&format!("{} '{}' not found", resource, name))
    }

    /// Output an error message without exiting.
    ///
    /// Use when you want to report an error but continue execution.
    pub fn error_continue(&self, error: &str) {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": false,
                    "error": error
                }))
                .unwrap()
            );
        } else {
            eprintln!("{}", error);
        }
    }

    /// Output an error with additional context data.
    pub fn error_with_data<T: Serialize>(&self, error: &str, data: T) {
        if self.json {
            let mut value = serde_json::to_value(&data).unwrap_or(serde_json::json!({}));
            if let Some(obj) = value.as_object_mut() {
                obj.insert("success".to_string(), serde_json::json!(false));
                obj.insert("error".to_string(), serde_json::json!(error));
            }
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
        } else {
            eprintln!("{}", error);
        }
    }

    /// Check if JSON output is enabled.
    pub fn is_json(&self) -> bool {
        self.json
    }
}
