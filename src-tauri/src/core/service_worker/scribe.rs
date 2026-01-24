//! Scribe service for processing documentation batches.
//!
//! Provides a unified interface for scribe operations. The service
//! handles processing batches locally or via remote workers transparently.

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::core::config::{run_dir, Config};
use crate::core::scribe::{self, ScribeBatchResult, ScribeError};
use crate::core::Files;

use super::types::{
    ServiceWorkerError, ServiceWorkerHandle, ServiceWorkerResult, ServiceWorkerType,
};

/// Service for processing scribe documentation batches.
///
/// Handles local vs remote execution internally - callers just call `process_batch()`.
pub struct ScribeService {
    config: Config,
    agent_command: Vec<String>,
    /// Handle to the remote scribe worker (if using remote)
    remote_worker: RwLock<Option<ServiceWorkerHandle>>,
}

impl ScribeService {
    /// Create a new scribe service
    pub fn new(config: Config, agent_command: Vec<String>) -> Self {
        Self {
            config,
            agent_command,
            remote_worker: RwLock::new(None),
        }
    }

    /// Create a new scribe service with default agent command
    pub fn with_config(config: Config) -> Self {
        let agent_command = crate::cli::config::get_agent_command();
        Self::new(config, agent_command)
    }

    /// Check if we should use a remote service worker.
    fn should_use_remote(&self) -> bool {
        let runner = self.config.service_workers.scribe_runner();
        match runner {
            None => false,                        // No config → local
            Some(r) if r == "local" => false,     // Explicit local
            Some(r) => self.config.has_runner(r), // Check if runner exists
        }
    }

    /// Process a scribe batch - handles local vs remote internally.
    ///
    /// This is the main entry point. Callers don't need to know about
    /// local vs remote - just call this method.
    pub async fn process_batch(&self, run_name: &str) -> Result<ScribeBatchResult, ScribeError> {
        if self.should_use_remote() {
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
        let config = self.config.clone();
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

        let endpoint = self
            .get_or_spawn_worker()
            .await
            .map_err(|e| ScribeError::AgentError(format!("Failed to get service worker: {}", e)))?;

        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/scribe/batch", endpoint))
            .json(&serde_json::json!({ "run_name": run_name }))
            .send()
            .await
            .map_err(|e| ScribeError::AgentError(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ScribeError::AgentError(format!(
                "Service worker returned error: {}",
                body
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ScribeError::AgentError(format!("Failed to parse response: {}", e)))?;

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

    /// Get or spawn a remote scribe worker.
    async fn get_or_spawn_worker(&self) -> ServiceWorkerResult<String> {
        // Check if we have a healthy existing worker
        {
            let guard = self.remote_worker.read().await;
            if let Some(handle) = guard.as_ref() {
                if self.health_check(handle).await {
                    return Ok(handle.endpoint.clone());
                }
            }
        }

        // Need to spawn a new worker
        let handle = self.spawn_worker().await?;
        let endpoint = handle.endpoint.clone();

        // Store the handle
        let mut guard = self.remote_worker.write().await;
        *guard = Some(handle);

        Ok(endpoint)
    }

    /// Spawn a scribe service worker.
    async fn spawn_worker(&self) -> ServiceWorkerResult<ServiceWorkerHandle> {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let runner_name = self
            .config
            .service_workers
            .scribe_runner()
            .ok_or(ServiceWorkerError::NoRunnerConfigured)?;

        info!(
            "[ScribeService] Spawning service worker on runner '{}'",
            runner_name
        );

        let exe = std::env::current_exe()
            .map_err(|e| ServiceWorkerError::SpawnFailed(format!("Failed to get exe: {}", e)))?;

        let idle_timeout = self.config.service_workers.scribe_idle_timeout();

        // Spawn the service worker process
        let mut child = Command::new(&exe)
            .arg("__service-worker")
            .arg("--type")
            .arg("scribe")
            .arg("--idle-timeout")
            .arg(idle_timeout.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ServiceWorkerError::SpawnFailed(format!("Failed to spawn: {}", e)))?;

        let pid = child.id().unwrap_or(0);

        // Read the port from stdout (first line)
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ServiceWorkerError::SpawnFailed("Failed to get stdout".into()))?;

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        // Wait for the ready message with timeout
        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            reader.read_line(&mut line),
        )
        .await;

        match read_result {
            Ok(Ok(0)) => Err(ServiceWorkerError::SpawnFailed(
                "Worker exited before ready".into(),
            )),
            Ok(Ok(_)) => {
                // Parse the JSON ready message
                // Expected format: {"status":"ready","port":12345}
                if let Ok(ready) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(port) = ready.get("port").and_then(|p| p.as_u64()) {
                        let endpoint = format!("http://127.0.0.1:{}", port);
                        debug!(
                            "[ScribeService] Worker ready on port {} (pid {})",
                            port, pid
                        );
                        return Ok(ServiceWorkerHandle::new(
                            ServiceWorkerType::Scribe,
                            endpoint,
                            "local".to_string(),
                            pid.to_string(),
                        ));
                    }
                }
                Err(ServiceWorkerError::SpawnFailed(format!(
                    "Invalid ready message: {}",
                    line.trim()
                )))
            }
            Ok(Err(e)) => Err(ServiceWorkerError::SpawnFailed(format!(
                "Read error: {}",
                e
            ))),
            Err(_) => {
                // Kill the child on timeout
                let _ = child.kill().await;
                Err(ServiceWorkerError::SpawnFailed(
                    "Timeout waiting for ready".into(),
                ))
            }
        }
    }

    /// Check if a service worker is healthy.
    async fn health_check(&self, handle: &ServiceWorkerHandle) -> bool {
        let url = handle.health_url();
        let client = reqwest::Client::new();

        match client
            .get(&url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => true,
            Ok(_) => {
                debug!("[ScribeService] Health check failed: bad status");
                false
            }
            Err(e) => {
                debug!("[ScribeService] Health check failed: {}", e);
                false
            }
        }
    }

    /// Stop the remote worker if running.
    pub async fn stop(&self) -> ServiceWorkerResult<()> {
        let handle = {
            let mut guard = self.remote_worker.write().await;
            guard.take()
        };

        if let Some(handle) = handle {
            if handle.runner_type == "local" {
                if let Ok(pid) = handle.runner_id.parse::<u32>() {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                    info!("[ScribeService] Stopped worker (pid {})", pid);
                }
            } else {
                warn!(
                    "[ScribeService] Cannot stop remote worker {} (type: {})",
                    handle.runner_id, handle.runner_type
                );
            }
        }

        Ok(())
    }
}

/// Create a ScribeService from config with default agent command.
pub fn create_scribe_service() -> ScribeService {
    let config = Config::load().map(|(c, _)| c).unwrap_or_default();
    ScribeService::with_config(config)
}
