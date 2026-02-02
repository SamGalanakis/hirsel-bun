//! Fly.io runner - spawns workers as ephemeral Fly Machines.
//!
//! Workers run in user-specified Docker images. The setup uses an init script
//! that downloads the hirsel binary, installs agent tools, and starts the worker.
//!
//! ## Fly Machines API
//!
//! This uses the Fly Machines REST API: https://fly.io/docs/machines/api/
//! Machines are created under a user-specified Fly app.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

use crate::core::constants::FLY_API_BASE;

use super::setup;
use super::{
    FlyHostConfig, Runner, RunnerError, RunnerResult, SpawnResult, WorkerHandle, WorkerSpawnConfig,
};

/// Fly.io runner - spawns workers as ephemeral Fly Machines.
pub struct FlyRunner {
    config: FlyHostConfig,
    image: String,
    client: reqwest::Client,
}

impl FlyRunner {
    /// Create a new Fly runner.
    pub fn new(config: FlyHostConfig, image: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            image,
            client,
        }
    }

    /// Get the API token from config or environment.
    fn api_token(&self) -> Result<String, RunnerError> {
        self.config
            .api_token
            .clone()
            .or_else(|| std::env::var("FLY_API_TOKEN").ok())
            .ok_or_else(|| {
                RunnerError::Config(
                    "FLY_API_TOKEN not set. Add to config or set FLY_API_TOKEN env var.".into(),
                )
            })
    }

    /// Generate a machine name for a worker.
    fn machine_name(run_name: &str, worker_name: &str) -> String {
        // Fly machine names must be lowercase alphanumeric with hyphens
        let name = format!("hirsel-{}-{}", run_name, worker_name);
        name.to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    }
}

#[async_trait]
impl Runner for FlyRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        let token = self.api_token()?;
        let coordinator_url = config.coordinator_url.as_ref().ok_or_else(|| {
            RunnerError::Config("coordinator_url required for Fly runner".to_string())
        })?;

        let machine_name = Self::machine_name(&config.run_name, &config.worker_name);

        info!(
            "Spawning Fly machine {} for worker {} in app {}",
            machine_name, config.worker_name, self.config.app
        );

        // Build environment variables for worker
        let mut env = HashMap::new();
        env.insert("HIRSEL_RUN".to_string(), config.run_name.clone());
        env.insert("HIRSEL_WORKER".to_string(), config.worker_name.clone());
        env.insert("HIRSEL_API_URL".to_string(), coordinator_url.clone());
        env.insert("HIRSEL_REMOTE".to_string(), "1".to_string());
        env.insert(
            "ACP_PERMISSION_MODE".to_string(),
            "bypassPermissions".to_string(),
        );

        // Add forwarded credentials and env vars
        for (k, v) in config.collect_env_vars() {
            env.insert(k, v);
        }

        // Generate init script
        let agent_command_json = serde_json::to_string(&config.agent_command)
            .unwrap_or_else(|_| "[\"claude\"]".to_string());

        let init_script = setup::generate_fly_init_script(
            coordinator_url,
            &config.run_name,
            &config.worker_name,
            &agent_command_json,
            config.is_leader,
            config.leader_name.as_deref(),
            config.teammates.as_deref(),
            config.assigned_task_id.as_deref(),
        );

        // Create machine request
        let request = CreateMachineRequest {
            name: Some(machine_name.clone()),
            config: MachineConfig {
                image: self.image.clone(),
                env,
                init: InitConfig {
                    exec: vec!["/bin/sh".into(), "-c".into(), init_script],
                },
                guest: GuestConfig {
                    cpu_kind: self.config.cpu_kind.clone(),
                    cpus: self.config.cpus as i32,
                    memory_mb: self.config.memory_mb as i32,
                },
                auto_destroy: self.config.auto_destroy,
            },
            region: self.config.region.clone(),
        };

        debug!("Creating Fly machine with config: {:?}", request);

        // POST to Fly Machines API
        let url = format!("{}/apps/{}/machines", FLY_API_BASE, self.config.app);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .json(&request)
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Fly API request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!("Fly API error {}: {}", status, body);
            return Err(RunnerError::SpawnFailed(format!(
                "Fly API error {}: {}",
                status, body
            )));
        }

        let machine: MachineResponse = resp
            .json()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to parse Fly response: {}", e)))?;

        info!(
            "Spawned Fly machine {} (id: {}) for worker {} in region {:?}",
            machine_name, machine.id, config.worker_name, machine.region
        );

        Ok(SpawnResult {
            pid: None, // Fly machines don't have local PIDs
            handle: WorkerHandle {
                worker_name: config.worker_name.clone(),
                runner_id: machine.id,
                runner_type: "fly".to_string(),
            },
        })
    }

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        let token = self.api_token()?;
        let machine_id = &handle.runner_id;

        info!("Stopping Fly machine {}", machine_id);

        // Stop the machine
        let url = format!(
            "{}/apps/{}/machines/{}/stop",
            FLY_API_BASE, self.config.app, machine_id
        );

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to stop machine: {}", e)))?;

        if resp.status().as_u16() == 404 {
            // Machine already gone
            debug!("Machine {} already destroyed", machine_id);
            return Ok(());
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(
                "Failed to stop machine {} ({}): {}",
                machine_id, status, body
            );
        }

        // If auto_destroy is not set, explicitly destroy the machine
        if !self.config.auto_destroy {
            let destroy_url = format!(
                "{}/apps/{}/machines/{}",
                FLY_API_BASE, self.config.app, machine_id
            );

            let _ = self
                .client
                .delete(&destroy_url)
                .bearer_auth(&token)
                .send()
                .await;
        }

        Ok(())
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
        let Ok(token) = self.api_token() else {
            return false;
        };

        let url = format!(
            "{}/apps/{}/machines/{}",
            FLY_API_BASE, self.config.app, handle.runner_id
        );

        let Ok(resp) = self.client.get(&url).bearer_auth(&token).send().await else {
            return false;
        };

        if !resp.status().is_success() {
            return false;
        }

        if let Ok(machine) = resp.json::<MachineResponse>().await {
            // Machine states: created, starting, started, stopping, stopped, destroying, destroyed
            matches!(machine.state.as_str(), "created" | "starting" | "started")
        } else {
            false
        }
    }

    fn runner_type(&self) -> &'static str {
        "fly"
    }

    fn is_ephemeral(&self) -> bool {
        true // Fly machines are destroyed on stop
    }

    async fn setup(&self) -> RunnerResult<()> {
        // Verify we have a valid token
        let _ = self.api_token()?;

        // Verify the app exists by listing machines (will fail if app doesn't exist)
        let token = self.api_token()?;
        let url = format!("{}/apps/{}/machines", FLY_API_BASE, self.config.app);

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| RunnerError::Config(format!("Failed to connect to Fly API: {}", e)))?;

        if resp.status().as_u16() == 404 {
            return Err(RunnerError::Config(format!(
                "Fly app '{}' not found. Create it with: fly apps create {}",
                self.config.app, self.config.app
            )));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RunnerError::Config(format!(
                "Failed to verify Fly app ({}): {}",
                status, body
            )));
        }

        info!("Verified Fly app '{}' exists", self.config.app);
        Ok(())
    }

    async fn cleanup(&self) -> RunnerResult<()> {
        // Machines with auto_destroy=true clean themselves up
        // Otherwise we rely on stop() being called for each worker
        Ok(())
    }
}

// =============================================================================
// Fly Machines API Types
// =============================================================================

/// Request to create a Fly machine.
#[derive(Debug, Serialize)]
struct CreateMachineRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    config: MachineConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
}

/// Machine configuration.
#[derive(Debug, Serialize)]
struct MachineConfig {
    image: String,
    env: HashMap<String, String>,
    init: InitConfig,
    guest: GuestConfig,
    auto_destroy: bool,
}

/// Init (entrypoint) configuration.
#[derive(Debug, Serialize)]
struct InitConfig {
    exec: Vec<String>,
}

/// Guest (VM size) configuration.
#[derive(Debug, Serialize)]
struct GuestConfig {
    cpu_kind: String,
    cpus: i32,
    memory_mb: i32,
}

/// Response from creating or getting a machine.
#[derive(Debug, Deserialize)]
struct MachineResponse {
    id: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    region: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machine_name_generation() {
        assert_eq!(
            FlyRunner::machine_name("my-run", "achilles"),
            "hirsel-my-run-achilles"
        );

        assert_eq!(
            FlyRunner::machine_name("Test Run", "Worker 1"),
            "hirsel-test-run-worker-1"
        );

        // Underscores become hyphens
        assert_eq!(
            FlyRunner::machine_name("my_run", "worker_1"),
            "hirsel-my-run-worker-1"
        );
    }

    #[test]
    fn test_fly_runner_type() {
        use super::super::FlyHostConfig;

        let config = FlyHostConfig {
            app: "test-app".to_string(),
            ..Default::default()
        };
        let runner = FlyRunner::new(config, "debian:bookworm-slim".to_string());
        assert_eq!(runner.runner_type(), "fly");
    }
}
