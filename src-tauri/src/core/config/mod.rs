//! Configuration system for hirsel.
//!
//! This module provides the configuration system for hirsel, including:
//! - Agent configuration (command, type detection)
//! - Authentication configuration (env vars, API keys, OAuth)
//! - Worker scaling configuration
//! - Remote worker configuration
//! - Main Config struct with all settings

mod agent;
mod git;
mod loader;
mod orchestrator;
mod paths;
mod saver;
mod storage;
mod types;
mod workers;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use thiserror::Error;

// Re-export all public types
pub use agent::AgentConfig;
pub use git::{GitConfig, GitProvider};
pub use orchestrator::{OrchestratorAccess, OrchestratorMode, OrchestratorProfile};
pub use paths::{global_db_path, hirsel_dir, list_runs, run_dir, run_exists, runs_dir};
pub use storage::{S3Config, StorageBackend, StorageConfig, StorageProvider};
pub use types::{get_agent_env_vars, AgentAuth, AgentType, AuthConfig, AuthMethod};
pub use workers::WorkerScale;

/// Context window sizes per model (in tokens)
pub const CONTEXT_WINDOWS: &[(&str, u32)] = &[
    ("claude-opus-4-5-20251101", 200_000),
    ("claude-sonnet-4-5-20251101", 200_000),
    ("claude-sonnet-4-20250514", 200_000),
    ("claude-3-5-sonnet-20241022", 200_000),
    ("claude-3-5-haiku-20241022", 200_000),
    ("claude-3-opus-20240229", 200_000),
    ("claude-3-sonnet-20240229", 200_000),
    ("claude-3-haiku-20240307", 200_000),
];

/// Default context window size for unknown models
pub const DEFAULT_CONTEXT_WINDOW: u32 = 200_000;

/// Get context window size for a model
pub fn get_context_window(model: &str) -> u32 {
    CONTEXT_WINDOWS
        .iter()
        .find(|(name, _)| *name == model)
        .map(|(_, size)| *size)
        .unwrap_or(DEFAULT_CONTEXT_WINDOW)
}

/// Configuration error type
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("No run selected. Set HIRSEL_RUN env var.")]
    NoRunSelected,

    #[error("Invalid TOML in config file {path}: {message}")]
    InvalidToml { path: PathBuf, message: String },

    #[error("Cannot read config file {path}: permission denied")]
    PermissionDenied { path: PathBuf },

    #[error("Failed to read config file {path}: {message}")]
    ReadError { path: PathBuf, message: String },

    #[error("Invalid workers format: '{value}'. Use a number like '4' for max workers")]
    InvalidWorkerScale { value: String },

    #[error("Worker count must be at least 1")]
    WorkerCountTooLow,

    #[error("Run name cannot be empty")]
    EmptyRunName,

    #[error("Run name cannot contain path separators")]
    RunNameHasPathSeparators,

    #[error("{0}")]
    ValidationError(String),
}

// Default value functions
fn default_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hirsel")
}

fn default_eval_timeout() -> u32 {
    1800
}

fn default_user_message_pause() -> String {
    "sender".to_string()
}

fn default_human_in_the_loop() -> bool {
    true
}

fn default_compaction_enabled() -> bool {
    true
}

fn default_compaction_threshold() -> Option<u32> {
    Some(10000)
}

fn default_compaction_keep_messages() -> u32 {
    40
}

fn default_auto_improve() -> bool {
    true
}

fn default_context_warning_threshold() -> f64 {
    0.5
}

fn default_coordinator_port() -> u16 {
    19700
}

fn default_profile() -> String {
    "local".to_string()
}

fn default_profiles() -> HashMap<String, OrchestratorProfile> {
    let mut profiles = HashMap::new();
    profiles.insert("local".to_string(), OrchestratorProfile::default());
    profiles
}

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_root")]
    pub root: PathBuf,
    pub run: Option<String>,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default = "default_eval_timeout")]
    pub eval_timeout: u32,

    #[serde(default)]
    pub auto_learn: bool,

    pub max_iterations: Option<u32>,

    #[serde(default = "default_user_message_pause")]
    pub user_message_pause: String,

    #[serde(default = "default_human_in_the_loop")]
    pub human_in_the_loop: bool,

    #[serde(default = "default_compaction_enabled")]
    pub compaction_enabled: bool,

    #[serde(default = "default_compaction_threshold")]
    pub compaction_threshold: Option<u32>,

    #[serde(default = "default_compaction_keep_messages")]
    pub compaction_keep_messages: u32,

    #[serde(default = "default_auto_improve")]
    pub auto_improve: bool,

    #[serde(default = "default_context_warning_threshold")]
    pub context_warning_threshold: f64,

    #[serde(default = "default_coordinator_port")]
    pub coordinator_port: u16,

    #[serde(default)]
    pub auth: AuthConfig,

    /// Named runners that can be referenced by workers
    #[serde(default)]
    pub runners: HashMap<String, crate::core::runner::RunnerConfig>,

    /// Default runner for workers (defaults to "local")
    #[serde(default)]
    pub default_runner: Option<String>,

    /// Per-worker runner assignments (worker_name -> runner_name)
    #[serde(default)]
    pub worker_runners: HashMap<String, String>,

    /// Default orchestrator profile name
    #[serde(default = "default_profile")]
    pub default_profile: String,

    /// Orchestrator profiles for local/remote connections
    #[serde(default = "default_profiles")]
    pub profiles: HashMap<String, OrchestratorProfile>,

    /// Git provider configuration
    #[serde(default)]
    pub git: GitConfig,

    /// Storage configuration for files and database
    #[serde(default)]
    pub storage: StorageConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: default_root(),
            run: None,
            agent: AgentConfig::default(),
            eval_timeout: default_eval_timeout(),
            auto_learn: true,
            max_iterations: None,
            user_message_pause: default_user_message_pause(),
            human_in_the_loop: default_human_in_the_loop(),
            compaction_enabled: default_compaction_enabled(),
            compaction_threshold: default_compaction_threshold(),
            compaction_keep_messages: default_compaction_keep_messages(),
            auto_improve: default_auto_improve(),
            context_warning_threshold: default_context_warning_threshold(),
            coordinator_port: default_coordinator_port(),
            auth: AuthConfig::default(),
            runners: HashMap::new(),
            default_runner: None,
            worker_runners: HashMap::new(),
            default_profile: default_profile(),
            profiles: default_profiles(),
            git: GitConfig::default(),
            storage: StorageConfig::default(),
        }
    }
}

impl Config {
    /// Create a new config from environment and config file
    pub fn load() -> Result<(Self, Vec<String>), ConfigError> {
        let mut config = Self::from_env();
        let config_path = config.config_file();
        let warnings = loader::load_config_file(&mut config, &config_path)?;
        Ok((config, warnings))
    }

    /// Create config from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(root) = env::var("HIRSEL_ROOT") {
            config.root = PathBuf::from(root);
        }

        if let Ok(run) = env::var("HIRSEL_RUN") {
            config.run = Some(run);
        }

        config
    }

    /// Path to runs directory
    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    /// Path to staging root directory
    pub fn staging_root(&self) -> PathBuf {
        PathBuf::from("/tmp/hirsel-staging")
    }

    /// Path to staging directory for a specific run
    pub fn staging_dir(&self, name: &str) -> PathBuf {
        self.staging_root().join(name)
    }

    /// Path to current run directory
    pub fn run_dir(&self) -> Result<PathBuf, ConfigError> {
        let run = self.run.as_ref().ok_or(ConfigError::NoRunSelected)?;
        Ok(self.runs_dir().join(run))
    }

    /// Path to database file
    pub fn db_path(&self) -> Result<PathBuf, ConfigError> {
        Ok(self.run_dir()?.join("hirsel.db"))
    }

    /// Path to work directory
    pub fn work_dir(&self) -> Result<PathBuf, ConfigError> {
        Ok(self.run_dir()?.join("work"))
    }

    /// Path to config file
    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// Path to log file
    pub fn log_file(&self) -> PathBuf {
        self.root.join("hirsel.log")
    }

    /// Validate run name
    pub fn validate_run_name(name: &str) -> Result<(), ConfigError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ConfigError::EmptyRunName);
        }
        if name.contains('/') || name.contains('\\') {
            return Err(ConfigError::RunNameHasPathSeparators);
        }
        Ok(())
    }

    /// Get the runner configuration for a specific worker
    ///
    /// Checks worker_runners first for a specific assignment,
    /// then falls back to default_runner, then to local.
    pub fn get_runner_for_worker(&self, worker_name: &str) -> crate::core::runner::RunnerConfig {
        // Check for specific worker assignment
        if let Some(runner_name) = self.worker_runners.get(worker_name) {
            if let Some(runner_config) = self.runners.get(runner_name) {
                return runner_config.clone();
            }
        }

        // Check default runner
        if let Some(ref default_name) = self.default_runner {
            if let Some(runner_config) = self.runners.get(default_name) {
                return runner_config.clone();
            }
        }

        // Fall back to local
        crate::core::runner::RunnerConfig::local()
    }

    /// Get a runner by name
    pub fn get_runner(&self, name: &str) -> Option<crate::core::runner::RunnerConfig> {
        if name == "local" {
            return Some(crate::core::runner::RunnerConfig::local());
        }
        self.runners.get(name).cloned()
    }

    /// Check if a named runner exists
    pub fn has_runner(&self, name: &str) -> bool {
        name == "local" || self.runners.contains_key(name)
    }

    /// Get all configured runner names
    pub fn runner_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.runners.keys().cloned().collect();
        names.insert(0, "local".to_string());
        names
    }

    /// Save the current configuration to config.toml
    pub fn save(&self) -> Result<(), ConfigError> {
        saver::save_config(self, &self.config_file())
    }

    /// Update general settings (max_workers, default_runner, etc.)
    pub fn update_general(
        &mut self,
        eval_timeout: Option<u32>,
        auto_learn: Option<bool>,
        max_iterations: Option<Option<u32>>,
        human_in_the_loop: Option<bool>,
        default_runner: Option<Option<String>>,
        coordinator_port: Option<u16>,
    ) {
        if let Some(v) = eval_timeout {
            self.eval_timeout = v;
        }
        if let Some(v) = auto_learn {
            self.auto_learn = v;
        }
        if let Some(v) = max_iterations {
            self.max_iterations = v;
        }
        if let Some(v) = human_in_the_loop {
            self.human_in_the_loop = v;
        }
        if let Some(v) = default_runner {
            self.default_runner = v;
        }
        if let Some(v) = coordinator_port {
            self.coordinator_port = v;
        }
    }

    /// Update agent settings
    pub fn update_agent(&mut self, command: Option<Vec<String>>) {
        if let Some(cmd) = command {
            self.agent.command = cmd;
        }
    }

    /// Update compaction settings
    pub fn update_compaction(
        &mut self,
        enabled: Option<bool>,
        threshold: Option<Option<u32>>,
        keep_messages: Option<u32>,
    ) {
        if let Some(v) = enabled {
            self.compaction_enabled = v;
        }
        if let Some(v) = threshold {
            self.compaction_threshold = v;
        }
        if let Some(v) = keep_messages {
            self.compaction_keep_messages = v;
        }
    }

    /// Update auth settings for a specific agent
    pub fn update_agent_auth(&mut self, agent: &str, auth: AgentAuth) {
        match agent {
            "claude" => self.auth.claude = Some(auth),
            "gemini" => self.auth.gemini = Some(auth),
            "codex" => self.auth.codex = Some(auth),
            "goose" => self.auth.goose = Some(auth),
            _ => {}
        }
    }

    /// Delete auth settings for a specific agent
    pub fn delete_agent_auth(&mut self, agent: &str) {
        match agent {
            "claude" => self.auth.claude = None,
            "gemini" => self.auth.gemini = None,
            "codex" => self.auth.codex = None,
            "goose" => self.auth.goose = None,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_run_name() {
        assert!(Config::validate_run_name("myrun").is_ok());
        assert!(Config::validate_run_name("my-run").is_ok());
        assert!(Config::validate_run_name("").is_err());
        assert!(Config::validate_run_name("my/run").is_err());
        assert!(Config::validate_run_name("my\\run").is_err());
    }
}
