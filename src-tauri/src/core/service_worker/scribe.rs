//! Scribe service for processing documentation batches.
//!
//! Provides a unified interface for scribe operations. The service
//! handles processing batches locally or via remote workers transparently.

use tracing::info;

use crate::core::config::{run_dir, Config};
use crate::core::http_client::ResponseExt;
use crate::core::scribe::{self, ScribeBatchResult, ScribeError};
use crate::core::Files;

use super::types::{ServiceWorkerBase, ServiceWorkerResult, ServiceWorkerType};

/// Service for processing scribe documentation batches.
///
/// Handles local vs remote execution internally - callers just call `process_batch()`.
pub struct ScribeService {
    base: ServiceWorkerBase,
    agent_command: Vec<String>,
}

impl ScribeService {
    /// Create a new scribe service
    pub fn new(config: Config, agent_command: Vec<String>) -> Self {
        Self {
            base: ServiceWorkerBase::new(config, ServiceWorkerType::Scribe),
            agent_command,
        }
    }

    /// Create a new scribe service with default agent command
    pub fn with_config(config: Config) -> Self {
        let agent_command = crate::cli::config::get_agent_command();
        Self::new(config, agent_command)
    }

    /// Process a scribe batch - handles local vs remote internally.
    ///
    /// This is the main entry point. Callers don't need to know about
    /// local vs remote - just call this method.
    pub async fn process_batch(&self, run_name: &str) -> Result<ScribeBatchResult, ScribeError> {
        if self.base.should_use_remote() {
            self.process_via_remote(run_name).await
        } else {
            self.process_locally(run_name).await
        }
    }

    /// Process batch locally by calling process_scribe_batch directly.
    ///
    /// Uses spawn_blocking + LocalSet because process_scribe_batch uses spawn_local
    /// for the ACP connection.
    async fn process_locally(&self, run_name: &str) -> Result<ScribeBatchResult, ScribeError> {
        info!(
            "[ScribeService] Processing batch locally for run '{}'",
            run_name
        );

        let run_dir = run_dir(run_name);
        let files = Files::new(&run_dir);
        let config = self.base.config().clone();
        let agent_command = self.agent_command.clone();

        // Run in a blocking task with a LocalSet because process_scribe_batch uses spawn_local
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| ScribeError::AgentError(format!("Failed to create runtime: {}", e)))?;

            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(scribe::process_scribe_batch(
                        &files,
                        &config,
                        &agent_command,
                    ))
                    .await
            })
        })
        .await
        .map_err(|e| ScribeError::AgentError(format!("Join error: {}", e)))?
    }

    /// Process batch via remote HTTP service worker.
    async fn process_via_remote(&self, run_name: &str) -> Result<ScribeBatchResult, ScribeError> {
        info!(
            "[ScribeService] Processing batch via remote service worker for run '{}'",
            run_name
        );

        let endpoint =
            self.base.get_or_spawn_worker().await.map_err(|e| {
                ScribeError::AgentError(format!("Failed to get service worker: {}", e))
            })?;

        let client = reqwest::Client::new();
        let result: serde_json::Value = client
            .post(format!("{}/scribe/batch", endpoint))
            .json(&serde_json::json!({ "run_name": run_name }))
            .send()
            .await
            .map_err(|e| ScribeError::AgentError(format!("HTTP request failed: {}", e)))?
            .json_or_error()
            .await
            .map_err(|e| ScribeError::AgentError(format!("Service worker error: {}", e)))?;

        if result.get("success").and_then(|v| v.as_bool()) != Some(true) {
            let error = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(ScribeError::AgentError(format!(
                "Scribe batch failed: {}",
                error
            )));
        }

        // Extract result fields
        let batch_id = result.get("batch_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let submissions_processed = result
            .get("submissions_processed")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        info!(
            "[ScribeService] Remote scribe completed for run '{}': batch={}, processed={}",
            run_name, batch_id, submissions_processed
        );

        Ok(ScribeBatchResult {
            batch_id,
            submissions_processed,
            success: true,
        })
    }

    /// Stop the remote worker if running.
    pub async fn stop(&self) -> ServiceWorkerResult<()> {
        self.base.stop().await
    }
}
