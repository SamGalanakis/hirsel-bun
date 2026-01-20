//! Legacy runner configuration types (for backwards compatibility with existing code).

use serde::{Deserialize, Serialize};

fn default_ssh_port() -> u16 {
    22
}

fn default_work_base() -> String {
    "/tmp/hirsel-remote".to_string()
}

fn default_auto_destroy() -> bool {
    true
}

fn default_idle_timeout() -> u32 {
    30
}

fn default_api_url() -> String {
    "https://api.sprites.dev".to_string()
}

/// Configuration for SSH runner (legacy format, used internally).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshRunnerConfig {
    /// SSH host (e.g., "user@server.example.com")
    pub host: String,
    /// Path to SSH private key (optional)
    #[serde(default)]
    pub ssh_key: Option<String>,
    /// SSH port
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    /// Base directory for work on remote
    #[serde(default = "default_work_base")]
    pub work_base: String,
    /// Display name for location
    #[serde(default)]
    pub location: Option<String>,
}

impl Default for SshRunnerConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            ssh_key: None,
            ssh_port: 22,
            work_base: "/tmp/hirsel-remote".to_string(),
            location: None,
        }
    }
}

/// Configuration for Sprites runner (legacy format, used internally).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteRunnerConfig {
    /// Sprites API token (stored directly)
    #[serde(default)]
    pub api_token: Option<String>,
    /// Base checkpoint to clone from (pre-configured image)
    #[serde(default)]
    pub base_checkpoint: Option<String>,
    /// Auto-destroy sprite when worker completes
    #[serde(default = "default_auto_destroy")]
    pub auto_destroy: bool,
    /// Max idle time before sleep (sprites auto-hibernate at 30s anyway)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u32,
    /// Sprites API base URL
    #[serde(default = "default_api_url")]
    pub api_url: String,
    /// Use push mode to send files directly to worker via HTTP.
    #[serde(default)]
    pub use_file_push: bool,
}

impl Default for SpriteRunnerConfig {
    fn default() -> Self {
        Self {
            api_token: None,
            base_checkpoint: None,
            auto_destroy: true,
            idle_timeout_secs: 30,
            api_url: "https://api.sprites.dev".to_string(),
            use_file_push: false,
        }
    }
}
