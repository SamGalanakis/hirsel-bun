//! Conflict resolver service for AI-assisted merge conflict resolution.
//!
//! Provides a unified interface for conflict resolution. The service
//! handles resolving conflicts locally or via remote workers transparently.

use std::path::Path;
use tracing::info;

use crate::core::config::Config;
use crate::core::conflict_resolver::{
    ConflictResolverError, ConflictResolverService as RawConflictResolver, ResolutionResult,
};
use crate::core::http_client::ResponseExt;

use super::types::{ServiceWorkerBase, ServiceWorkerResult, ServiceWorkerType};

/// Service for resolving merge conflicts via AI agent.
///
/// Handles local vs remote execution internally - callers just call `resolve_conflicts()`.
pub struct ConflictResolverServiceWrapper {
    base: ServiceWorkerBase,
    agent_command: Vec<String>,
}

impl ConflictResolverServiceWrapper {
    /// Create a new conflict resolver service
    pub fn new(config: Config, agent_command: Vec<String>) -> Self {
        Self {
            base: ServiceWorkerBase::new(config, ServiceWorkerType::ConflictResolver),
            agent_command,
        }
    }

    /// Create a new conflict resolver service with default agent command
    pub fn with_config(config: Config) -> Self {
        let agent_command = crate::cli::config::get_agent_command();
        Self::new(config, agent_command)
    }

    /// Resolve conflicts - handles local vs remote internally.
    ///
    /// This is the main entry point. Callers don't need to know about
    /// local vs remote - just call this method.
    pub async fn resolve_conflicts(
        &self,
        work_dir: &Path,
        conflicts: Vec<String>,
        context: &str,
    ) -> Result<ResolutionResult, ConflictResolverError> {
        if self.base.should_use_remote() {
            self.resolve_via_remote(work_dir, conflicts, context).await
        } else {
            self.resolve_locally(work_dir, conflicts, context).await
        }
    }

    /// Resolve conflicts locally by calling the resolver directly.
    ///
    /// Uses spawn_blocking + LocalSet because the resolver uses spawn_local
    /// for the ACP connection.
    async fn resolve_locally(
        &self,
        work_dir: &Path,
        conflicts: Vec<String>,
        context: &str,
    ) -> Result<ResolutionResult, ConflictResolverError> {
        info!(
            "[ConflictResolverService] Resolving {} conflicts locally in {}",
            conflicts.len(),
            work_dir.display()
        );

        let agent_command = self.agent_command.clone();
        let work_dir = work_dir.to_path_buf();
        let context = context.to_string();

        // Run in a blocking task with a LocalSet because the resolver uses spawn_local
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| {
                    ConflictResolverError::AgentError(format!("Failed to create runtime: {}", e))
                })?;

            rt.block_on(async {
                let resolver = RawConflictResolver::new(agent_command);
                tokio::task::LocalSet::new()
                    .run_until(resolver.resolve_conflicts(&work_dir, conflicts, &context))
                    .await
            })
        })
        .await
        .map_err(|e| ConflictResolverError::AgentError(format!("Join error: {}", e)))?
    }

    /// Resolve conflicts via remote HTTP service worker.
    async fn resolve_via_remote(
        &self,
        work_dir: &Path,
        conflicts: Vec<String>,
        context: &str,
    ) -> Result<ResolutionResult, ConflictResolverError> {
        info!(
            "[ConflictResolverService] Resolving {} conflicts via remote service worker",
            conflicts.len()
        );

        let endpoint = self.base.get_or_spawn_worker().await.map_err(|e| {
            ConflictResolverError::AgentError(format!("Failed to get service worker: {}", e))
        })?;

        let client = reqwest::Client::new();
        let result: serde_json::Value = client
            .post(format!("{}/conflict-resolver/resolve", endpoint))
            .json(&serde_json::json!({
                "work_dir": work_dir.to_string_lossy(),
                "conflicts": conflicts,
                "context": context,
            }))
            .timeout(std::time::Duration::from_secs(600)) // 10 minute timeout
            .send()
            .await
            .map_err(|e| ConflictResolverError::AgentError(format!("HTTP request failed: {}", e)))?
            .json_or_error()
            .await
            .map_err(|e| {
                ConflictResolverError::AgentError(format!("Service worker error: {}", e))
            })?;

        if result.get("success").and_then(|v| v.as_bool()) != Some(true) {
            let error = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(ConflictResolverError::AgentError(format!(
                "Conflict resolution failed: {}",
                error
            )));
        }

        let files_resolved = result
            .get("files_resolved")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        info!(
            "[ConflictResolverService] Remote resolution completed: {} files resolved",
            files_resolved
        );

        Ok(ResolutionResult {
            files_resolved,
            success: true,
        })
    }

    /// Stop the remote worker if running.
    pub async fn stop(&self) -> ServiceWorkerResult<()> {
        self.base.stop().await
    }
}
