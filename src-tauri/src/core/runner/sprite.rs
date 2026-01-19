//! Sprite runner implementation - spawns workers on Sprites.dev cloud VMs.
//!
//! Sprites are lightweight, persistent VMs powered by Firecracker that hibernate
//! when idle and wake instantly when needed. Each worker gets its own sprite
//! for isolation.
//!
//! API Reference: https://docs.sprites.dev/

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use super::{
    Runner, RunnerError, RunnerResult, SpawnResult, SpriteRunnerConfig, WorkerHandle,
    WorkerSpawnConfig,
};

/// Create a tarball of a project directory, excluding common build artifacts
fn create_project_tarball(project_path: &Path) -> Result<Vec<u8>, std::io::Error> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;
    use walkdir::WalkDir;

    let mut buffer = Vec::new();
    let encoder = GzEncoder::new(&mut buffer, Compression::fast());
    let mut builder = Builder::new(encoder);

    // Walk the project directory, excluding common build artifacts
    for entry in WalkDir::new(project_path)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Exclude common build artifacts and hidden dirs
            !matches!(
                name.as_ref(),
                "node_modules"
                    | "target"
                    | ".git"
                    | ".hirsel"
                    | "__pycache__"
                    | ".venv"
                    | "venv"
                    | ".tox"
                    | "dist"
                    | "build"
                    | ".next"
                    | ".nuxt"
            )
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == project_path {
            continue;
        }

        let relative_path = path.strip_prefix(project_path).unwrap_or(path);

        if entry.file_type().is_file() {
            builder
                .append_path_with_name(path, relative_path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        } else if entry.file_type().is_dir() {
            builder
                .append_dir(relative_path, path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        }
    }

    builder
        .into_inner()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .finish()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    debug!("Created tarball: {} bytes", buffer.len());
    Ok(buffer)
}

/// Sprites API client
pub struct SpritesClient {
    token: String,
    base_url: String,
    http: reqwest::Client,
}

/// Sprite status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sprite {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
}

/// Create sprite request
#[derive(Debug, Serialize)]
struct CreateSpriteRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkpoint: Option<String>,
}

/// Exec command request
#[derive(Debug, Serialize)]
struct ExecRequest {
    command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
}

/// Exec command response
#[derive(Debug, Deserialize)]
pub struct ExecResponse {
    pub session_id: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
}

/// Checkpoint response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub comment: Option<String>,
    pub created_at: String,
}

impl SpritesClient {
    /// Create a new Sprites API client
    pub fn new(token: String, base_url: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            token,
            base_url: base_url.unwrap_or_else(|| "https://api.sprites.dev/v1".to_string()),
            http,
        }
    }

    /// Build authorization headers
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.token)).expect("Invalid token format"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    /// Create a new sprite
    pub async fn create(&self, name: &str) -> RunnerResult<Sprite> {
        let url = format!("{}/sprites", self.base_url);
        let body = CreateSpriteRequest {
            name: name.to_string(),
            image: None,
            checkpoint: None,
        };

        let response = self
            .http
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to create sprite: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RunnerError::Api(format!(
                "Failed to create sprite ({}): {}",
                status, text
            )));
        }

        response
            .json::<Sprite>()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to parse sprite response: {}", e)))
    }

    /// Create a sprite from a checkpoint
    pub async fn create_from_checkpoint(
        &self,
        name: &str,
        checkpoint_id: &str,
    ) -> RunnerResult<Sprite> {
        let url = format!("{}/sprites", self.base_url);
        let body = CreateSpriteRequest {
            name: name.to_string(),
            image: None,
            checkpoint: Some(checkpoint_id.to_string()),
        };

        let response = self
            .http
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to create sprite: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RunnerError::Api(format!(
                "Failed to create sprite from checkpoint ({}): {}",
                status, text
            )));
        }

        response
            .json::<Sprite>()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to parse sprite response: {}", e)))
    }

    /// Get sprite status
    pub async fn status(&self, name: &str) -> RunnerResult<Sprite> {
        let url = format!("{}/sprites/{}", self.base_url, name);

        let response = self
            .http
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to get sprite status: {}", e)))?;

        if response.status().as_u16() == 404 {
            return Err(RunnerError::WorkerNotFound(name.to_string()));
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RunnerError::Api(format!(
                "Failed to get sprite status ({}): {}",
                status, text
            )));
        }

        response
            .json::<Sprite>()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to parse sprite response: {}", e)))
    }

    /// Destroy a sprite
    pub async fn destroy(&self, name: &str) -> RunnerResult<()> {
        let url = format!("{}/sprites/{}", self.base_url, name);

        let response = self
            .http
            .delete(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to destroy sprite: {}", e)))?;

        if response.status().as_u16() == 404 {
            // Already gone
            return Ok(());
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RunnerError::Api(format!(
                "Failed to destroy sprite ({}): {}",
                status, text
            )));
        }

        Ok(())
    }

    /// Execute a command on a sprite (simple HTTP POST, non-streaming)
    pub async fn exec(
        &self,
        name: &str,
        command: &[String],
        env: Option<std::collections::HashMap<String, String>>,
        cwd: Option<String>,
    ) -> RunnerResult<ExecResponse> {
        let url = format!("{}/sprites/{}/exec", self.base_url, name);
        let body = ExecRequest {
            command: command.to_vec(),
            env,
            cwd,
        };

        let response = self
            .http
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to execute command: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RunnerError::Api(format!(
                "Failed to execute command ({}): {}",
                status, text
            )));
        }

        response
            .json::<ExecResponse>()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to parse exec response: {}", e)))
    }

    /// Execute a command in the background (detached)
    /// Returns immediately after spawning, use the session_id to track
    pub async fn exec_detached(
        &self,
        name: &str,
        command: &[String],
        env: Option<std::collections::HashMap<String, String>>,
        cwd: Option<String>,
    ) -> RunnerResult<String> {
        // For detached execution, we wrap the command in nohup
        let wrapped_command = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "nohup {} > /tmp/worker.log 2>&1 & echo $!",
                command.join(" ")
            ),
        ];

        let response = self.exec(name, &wrapped_command, env, cwd).await?;

        // The stdout should contain the PID
        let pid = response
            .stdout
            .as_ref()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| response.session_id.clone());

        Ok(pid)
    }

    /// Create a checkpoint of the sprite
    pub async fn checkpoint(&self, name: &str, comment: &str) -> RunnerResult<Checkpoint> {
        let url = format!("{}/sprites/{}/checkpoints", self.base_url, name);

        #[derive(Serialize)]
        struct CheckpointRequest {
            comment: String,
        }

        let body = CheckpointRequest {
            comment: comment.to_string(),
        };

        let response = self
            .http
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to create checkpoint: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RunnerError::Api(format!(
                "Failed to create checkpoint ({}): {}",
                status, text
            )));
        }

        response
            .json::<Checkpoint>()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to parse checkpoint response: {}", e)))
    }

    /// Restore from a checkpoint
    pub async fn restore(&self, name: &str, checkpoint_id: &str) -> RunnerResult<()> {
        let url = format!(
            "{}/sprites/{}/checkpoints/{}/restore",
            self.base_url, name, checkpoint_id
        );

        let response = self
            .http
            .post(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to restore checkpoint: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RunnerError::Api(format!(
                "Failed to restore checkpoint ({}): {}",
                status, text
            )));
        }

        Ok(())
    }

    /// List exec sessions
    pub async fn list_exec_sessions(&self, name: &str) -> RunnerResult<Vec<String>> {
        let url = format!("{}/sprites/{}/exec", self.base_url, name);

        let response = self
            .http
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to list exec sessions: {}", e)))?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        #[derive(Deserialize)]
        struct SessionList {
            sessions: Vec<String>,
        }

        response
            .json::<SessionList>()
            .await
            .map(|r| r.sessions)
            .map_err(|e| RunnerError::Api(format!("Failed to parse exec sessions: {}", e)))
    }

    /// Kill an exec session
    pub async fn kill_exec_session(&self, name: &str, session_id: &str) -> RunnerResult<()> {
        let url = format!(
            "{}/sprites/{}/exec/{}/kill",
            self.base_url, name, session_id
        );

        let response = self
            .http
            .post(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| RunnerError::Api(format!("Failed to kill exec session: {}", e)))?;

        if !response.status().is_success() && response.status().as_u16() != 404 {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            warn!("Failed to kill exec session ({}): {}", status, text);
        }

        Ok(())
    }
}

/// Sprite runner - spawns workers on Sprites.dev cloud VMs
pub struct SpriteRunner {
    config: SpriteRunnerConfig,
    client: SpritesClient,
}

impl SpriteRunner {
    /// Create a new sprite runner
    pub fn new(config: SpriteRunnerConfig) -> Self {
        // Get token from config, or fall back to SPRITES_TOKEN env var for backwards compatibility
        let token = config.api_token.clone().unwrap_or_else(|| {
            std::env::var("SPRITES_TOKEN").unwrap_or_else(|_| {
                warn!("No Sprites API token configured and SPRITES_TOKEN env var not set");
                String::new()
            })
        });

        let client = SpritesClient::new(token, Some(config.api_url.clone()));

        Self { config, client }
    }

    /// Generate sprite name for a worker
    fn sprite_name(run_name: &str, worker_name: &str) -> String {
        // Sprites names must be lowercase alphanumeric with hyphens
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
impl Runner for SpriteRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        let sprite_name = Self::sprite_name(&config.run_name, &config.worker_name);

        // Step 1: Create sprite (or from checkpoint if configured)
        info!(
            "Creating sprite {} for worker {}",
            sprite_name, config.worker_name
        );

        let sprite = if let Some(ref checkpoint) = self.config.base_checkpoint {
            self.client
                .create_from_checkpoint(&sprite_name, checkpoint)
                .await?
        } else {
            self.client.create(&sprite_name).await?
        };

        debug!(
            "Sprite {} created with status: {}",
            sprite.name, sprite.status
        );

        // Step 2: Setup Tailscale if auth key provided
        if let Some(ref authkey) = config.tailscale_authkey {
            info!("Setting up Tailscale on sprite {}", sprite_name);
            let tailscale_commands = vec![
                // Install Tailscale if not present
                "which tailscale || curl -fsSL https://tailscale.com/install.sh | sh".to_string(),
                // Connect to tailnet (--accept-routes to use exit nodes if configured)
                format!(
                    "tailscale status || tailscale up --authkey={} --accept-routes --hostname={}",
                    authkey, sprite_name
                ),
            ];

            for cmd in tailscale_commands {
                debug!("Running Tailscale setup: {}", cmd);
                let result = self
                    .client
                    .exec(
                        &sprite_name,
                        &["sh".to_string(), "-c".to_string(), cmd.clone()],
                        None,
                        None,
                    )
                    .await;

                if let Err(e) = result {
                    warn!("Tailscale setup command failed: {} - {}", cmd, e);
                    // Don't fail the whole spawn - Tailscale might already be set up
                }
            }
        }

        // Step 3: Sync work directory (now step 3 after Tailscale)
        let work_dir = "/home/sprite/work";
        let use_file_push = self.config.use_file_push && config.tailscale_authkey.is_some();

        if !use_file_push {
            // Pull mode - worker downloads files from coordinator or git
            let setup_commands = if let Some(ref project_url) = config.project_url {
                // Git clone mode
                vec![
                    format!("mkdir -p {}", work_dir),
                    format!(
                        "if [ -d {}/.git ]; then cd {} && git fetch origin && git reset --hard origin/HEAD; else git clone {} {}; fi",
                        work_dir, work_dir, project_url, work_dir
                    ),
                    format!("mkdir -p {}/chats", work_dir),
                ]
            } else if let Some(ref coordinator_url) = config.coordinator_url {
                // Tarball download mode - get files from coordinator API
                let files_url = format!("{}/api/runs/{}/files", coordinator_url, config.run_name);
                vec![
                    format!("mkdir -p {}", work_dir),
                    format!(
                        "cd {} && curl -sS -H 'Authorization: Bearer $HIRSEL_API_KEY' '{}' | tar -xzf -",
                        work_dir, files_url
                    ),
                    format!("cd {} && git init && git add . && git commit -m 'Initial import from server' 2>/dev/null || true", work_dir),
                    format!("mkdir -p {}/chats", work_dir),
                ]
            } else {
                return Err(RunnerError::Config(
                    "Either project_url or coordinator_url required for sprite runner".to_string(),
                ));
            };

            for cmd in setup_commands {
                info!("Running setup command: {}", cmd);
                let result = self
                    .client
                    .exec(
                        &sprite_name,
                        &["sh".to_string(), "-c".to_string(), cmd.clone()],
                        None,
                        None,
                    )
                    .await;

                if let Err(e) = result {
                    error!("Setup command failed: {} - {}", cmd, e);
                    // Clean up sprite on failure
                    let _ = self.client.destroy(&sprite_name).await;
                    return Err(RunnerError::SetupFailed(format!(
                        "Setup command failed: {}",
                        e
                    )));
                }
            }
        } else {
            // Push mode - just create work directory, files will be pushed after worker starts
            info!("Push mode enabled - creating work directory only");
            let result = self
                .client
                .exec(
                    &sprite_name,
                    &["mkdir".to_string(), "-p".to_string(), work_dir.to_string()],
                    None,
                    None,
                )
                .await;

            if let Err(e) = result {
                let _ = self.client.destroy(&sprite_name).await;
                return Err(RunnerError::SetupFailed(format!(
                    "Failed to create work directory: {}",
                    e
                )));
            }
        }

        // Step 3: Start worker process in background
        let coordinator_url = config.coordinator_url.as_ref().ok_or_else(|| {
            RunnerError::Config("coordinator_url required for sprite runner".to_string())
        })?;

        // Build environment variables
        let mut env = std::collections::HashMap::new();
        env.insert("HIRSEL_RUN".to_string(), config.run_name.clone());
        env.insert("HIRSEL_WORKER".to_string(), config.worker_name.clone());
        env.insert("HIRSEL_API_URL".to_string(), coordinator_url.clone());
        env.insert("HIRSEL_REMOTE".to_string(), "1".to_string());
        env.insert(
            "ACP_PERMISSION_MODE".to_string(),
            "bypassPermissions".to_string(),
        );

        if let Some(ref extra_env) = config.env_vars {
            for (k, v) in extra_env {
                env.insert(k.clone(), v.clone());
            }
        }

        // Build worker command
        let agent_command_json = serde_json::to_string(&config.agent_command)
            .unwrap_or_else(|_| "[\"claude-code-acp\"]".to_string());

        let mut worker_args = format!(
            "hirsel __remote-worker --api-url '{}' --run-name '{}' --worker-name '{}' --work-dir '{}' --spec '{}/spec.md' --agent-command '{}'",
            coordinator_url,
            config.run_name,
            config.worker_name,
            work_dir,
            work_dir,
            agent_command_json.replace('\'', "'\\''")
        );

        if config.is_leader {
            worker_args.push_str(" --is-leader");
        }

        if let Some(ref leader) = config.leader_name {
            worker_args.push_str(&format!(" --leader-name '{}'", leader));
        }

        if let Some(ref teammates) = config.teammates {
            if !teammates.is_empty() {
                worker_args.push_str(&format!(" --teammates '{}'", teammates.join(",")));
            }
        }

        // Add file receiver flag if push mode enabled
        if use_file_push {
            worker_args.push_str(" --wait-for-files");
        }

        info!("Starting worker process on sprite {}", sprite_name);

        let pid = self
            .client
            .exec_detached(
                &sprite_name,
                &["sh".to_string(), "-c".to_string(), worker_args],
                Some(env),
                Some(work_dir.to_string()),
            )
            .await?;

        // If push mode, push files to worker
        if use_file_push {
            info!(
                "Push mode: sending files to worker {} via Tailscale",
                sprite_name
            );

            // Create tarball from work directory
            let tarball = create_project_tarball(&config.work_dir).map_err(|e| {
                RunnerError::SetupFailed(format!("Failed to create tarball: {}", e))
            })?;

            info!("Created tarball: {} bytes", tarball.len());

            // Wait for worker's file receiver to be ready
            let worker_url = format!("http://{}:19800", sprite_name);
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| RunnerError::Api(format!("Failed to create HTTP client: {}", e)))?;

            let mut ready = false;
            for attempt in 1..=30 {
                debug!(
                    "Checking worker file receiver (attempt {}/30): {}/health",
                    attempt, worker_url
                );
                match client.get(format!("{}/health", worker_url)).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        info!("Worker file receiver is ready");
                        ready = true;
                        break;
                    }
                    Ok(resp) => {
                        debug!("Health check returned {}", resp.status());
                    }
                    Err(e) => {
                        debug!("Health check failed: {}", e);
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }

            if !ready {
                let _ = self.client.destroy(&sprite_name).await;
                return Err(RunnerError::SetupFailed(
                    "Worker file receiver did not become ready".to_string(),
                ));
            }

            // Push tarball to worker
            info!("Pushing tarball to worker...");
            let push_resp = client
                .post(format!("{}/upload", worker_url))
                .body(tarball)
                .send()
                .await
                .map_err(|e| RunnerError::Api(format!("Failed to push files: {}", e)))?;

            if !push_resp.status().is_success() {
                let status = push_resp.status();
                let body = push_resp.text().await.unwrap_or_default();
                let _ = self.client.destroy(&sprite_name).await;
                return Err(RunnerError::SetupFailed(format!(
                    "Failed to push files ({}): {}",
                    status, body
                )));
            }

            info!("Files pushed successfully");
        }

        info!(
            "Worker {} started on sprite {} (PID: {})",
            config.worker_name, sprite_name, pid
        );

        Ok(SpawnResult {
            handle: WorkerHandle {
                worker_name: config.worker_name.clone(),
                runner_id: sprite_name,
                runner_type: "sprite".to_string(),
            },
            pid: pid.parse().ok(),
        })
    }

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        let sprite_name = &handle.runner_id;

        // Kill any running processes
        info!("Stopping worker processes on sprite {}", sprite_name);
        let _ = self
            .client
            .exec(
                sprite_name,
                &["pkill".to_string(), "-f".to_string(), "hirsel".to_string()],
                None,
                None,
            )
            .await;

        // Optionally destroy the sprite
        if self.config.auto_destroy {
            info!("Destroying sprite {}", sprite_name);
            self.client.destroy(sprite_name).await?;
        }

        Ok(())
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
        let sprite_name = &handle.runner_id;

        // Check if sprite exists and is running
        match self.client.status(sprite_name).await {
            Ok(_sprite) => {
                // Check if worker process is running
                let result = self
                    .client
                    .exec(
                        sprite_name,
                        &["pgrep".to_string(), "-f".to_string(), "hirsel".to_string()],
                        None,
                        None,
                    )
                    .await;

                match result {
                    Ok(exec) => exec.exit_code == Some(0),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }

    fn runner_type(&self) -> &'static str {
        "sprite"
    }

    async fn setup(&self) -> RunnerResult<()> {
        // Verify we have a valid token
        if self.client.token.is_empty() {
            return Err(RunnerError::Config(
                "Sprites API token not configured. Add your token in Settings > Runners."
                    .to_string(),
            ));
        }
        Ok(())
    }

    async fn cleanup(&self) -> RunnerResult<()> {
        // Sprites auto-hibernate, no cleanup needed
        Ok(())
    }

    async fn get_logs(&self, handle: &WorkerHandle, lines: usize) -> RunnerResult<String> {
        let sprite_name = &handle.runner_id;

        let result = self
            .client
            .exec(
                sprite_name,
                &[
                    "tail".to_string(),
                    "-n".to_string(),
                    lines.to_string(),
                    "/tmp/worker.log".to_string(),
                ],
                None,
                None,
            )
            .await?;

        Ok(result.stdout.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_name_generation() {
        assert_eq!(
            SpriteRunner::sprite_name("my-run", "achilles"),
            "hirsel-my-run-achilles"
        );

        assert_eq!(
            SpriteRunner::sprite_name("Test Run", "Worker 1"),
            "hirsel-test-run-worker-1"
        );
    }

    #[test]
    fn test_sprite_runner_type() {
        let config = SpriteRunnerConfig::default();
        let runner = SpriteRunner::new(config);
        assert_eq!(runner.runner_type(), "sprite");
    }
}
