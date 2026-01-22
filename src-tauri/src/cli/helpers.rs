//! CLI helper utilities for orchestrator integration.
//!
//! This module provides helpers for CLI commands to use the Orchestrator trait,
//! enabling both local and remote operation through the same code paths.

use crate::core::orchestrator::{create_orchestrator, Orchestrator, OrchestratorError};
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
