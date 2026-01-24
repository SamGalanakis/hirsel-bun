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
pub mod paths;
mod saver;
mod storage;
pub mod store;
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
pub use store::{ConfigStore, ConfigStoreError, PartialConfig};
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

    #[error("Config store error: {0}")]
    Store(#[from] store::ConfigStoreError),
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

fn default_context_warning_threshold() -> f64 {
    0.5
}

fn default_coordinator_port() -> u16 {
    19700
}

fn default_profile() -> String {
    "local".to_string()
}

fn default_allow_local_workers() -> bool {
    true
}

fn default_scribe_enabled() -> bool {
    true
}

fn default_scribe_batch_window() -> u32 {
    3
}

fn default_scribe_idle_timeout() -> u32 {
    300 // 5 minutes
}

fn default_gyp_idle_timeout() -> u32 {
    600 // 10 minutes
}

fn default_scribe_docs_path() -> String {
    "docs".to_string()
}

fn default_scribe_persist_docs_changes() -> bool {
    true
}

fn default_profiles() -> HashMap<String, OrchestratorProfile> {
    let mut profiles = HashMap::new();
    profiles.insert("local".to_string(), OrchestratorProfile::default());
    profiles
}

/// Configuration for a single service worker (scribe or gyp)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceWorkerConfig {
    /// Runner name override for this service worker
    #[serde(default)]
    pub runner: Option<String>,
    /// Idle timeout in seconds before the worker self-terminates
    #[serde(default)]
    pub idle_timeout_seconds: Option<u32>,
}

/// Configuration for service workers (scribe, gyp)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceWorkersConfig {
    /// Default runner for all service workers (falls back to local)
    #[serde(default)]
    pub runner: Option<String>,
    /// Scribe service worker configuration
    #[serde(default)]
    pub scribe: ServiceWorkerConfig,
    /// Gyp service worker configuration
    #[serde(default)]
    pub gyp: ServiceWorkerConfig,
}

impl ServiceWorkersConfig {
    /// Get the effective runner for scribe
    pub fn scribe_runner(&self) -> Option<&str> {
        self.scribe.runner.as_deref().or(self.runner.as_deref())
    }

    /// Get the effective runner for gyp
    pub fn gyp_runner(&self) -> Option<&str> {
        self.gyp.runner.as_deref().or(self.runner.as_deref())
    }

    /// Get the idle timeout for scribe in seconds
    pub fn scribe_idle_timeout(&self) -> u32 {
        self.scribe
            .idle_timeout_seconds
            .unwrap_or(default_scribe_idle_timeout())
    }

    /// Get the idle timeout for gyp in seconds
    pub fn gyp_idle_timeout(&self) -> u32 {
        self.gyp
            .idle_timeout_seconds
            .unwrap_or(default_gyp_idle_timeout())
    }
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

    /// Whether to allow local workers (default: true)
    /// Set to false on remote coordinators (e.g., Fly.io) where local workers don't make sense
    #[serde(default = "default_allow_local_workers")]
    pub allow_local_workers: bool,

    /// Whether to enable the scribe system for documentation updates (default: true)
    #[serde(default = "default_scribe_enabled")]
    pub scribe_enabled: bool,

    /// How long to wait (in seconds) for more submissions before processing a scribe batch (default: 3)
    #[serde(default = "default_scribe_batch_window")]
    pub scribe_batch_window_seconds: u32,

    /// Service workers configuration (scribe, gyp)
    #[serde(default)]
    pub service_workers: ServiceWorkersConfig,

    /// Path to documentation directory relative to workspace (default: "docs")
    #[serde(default = "default_scribe_docs_path")]
    pub scribe_docs_path: String,

    /// Whether to persist scribe documentation changes back to workspace on delivery (default: true)
    #[serde(default = "default_scribe_persist_docs_changes")]
    pub scribe_persist_docs_changes: bool,
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
            allow_local_workers: default_allow_local_workers(),
            scribe_enabled: default_scribe_enabled(),
            scribe_batch_window_seconds: default_scribe_batch_window(),
            service_workers: ServiceWorkersConfig::default(),
            scribe_docs_path: default_scribe_docs_path(),
            scribe_persist_docs_changes: default_scribe_persist_docs_changes(),
        }
    }
}

impl Config {
    /// Create a new config from environment, database, and config file
    ///
    /// Load priority (later sources override earlier):
    /// 1. Default values
    /// 2. Database (`~/.hirsel/hirsel.db`)
    /// 3. Config file (`~/.hirsel/config.toml`) - also saves to DB
    /// 4. Environment variables (always win)
    ///
    /// Environment variables:
    /// - `HIRSEL_ROOT`: Override the hirsel root directory (default: ~/.hirsel)
    /// - `HIRSEL_RUN`: Set the current run name
    /// - `HIRSEL_ALLOW_LOCAL_WORKERS`: Override allow_local_workers setting
    pub fn load() -> Result<(Self, Vec<String>), ConfigError> {
        let mut config = Self::default();
        let mut warnings = Vec::new();

        // Apply env vars for root path first (needed for DB path)
        if let Ok(root) = env::var("HIRSEL_ROOT") {
            config.root = PathBuf::from(root);
        }

        // 1. Try loading from DB
        match ConfigStore::open() {
            Ok(store) => {
                if let Some(partial) = store.load_config()? {
                    config.merge_from(partial);
                }

                // 2. Check for config file override
                let config_path = config.config_file();
                if config_path.exists() {
                    let file_warnings = loader::load_config_file(&mut config, &config_path)?;
                    warnings.extend(file_warnings);

                    // Save file config to DB (one-time migration or update)
                    if let Err(e) = store.save_config(&config) {
                        warnings.push(format!("Failed to save config to database: {}", e));
                    }
                }
            }
            Err(e) => {
                warnings.push(format!("Failed to open config store: {}", e));

                // Fall back to file-only loading
                let config_path = config.config_file();
                if config_path.exists() {
                    let file_warnings = loader::load_config_file(&mut config, &config_path)?;
                    warnings.extend(file_warnings);
                }
            }
        }

        // 3. Apply environment overrides (always win)
        if let Ok(run) = env::var("HIRSEL_RUN") {
            config.run = Some(run);
        }
        if let Ok(val) = env::var("HIRSEL_ALLOW_LOCAL_WORKERS") {
            config.allow_local_workers = val != "0" && val.to_lowercase() != "false";
        }

        Ok((config, warnings))
    }

    /// Merge values from a PartialConfig, overwriting existing values
    pub fn merge_from(&mut self, partial: PartialConfig) {
        if let Some(agent) = partial.agent {
            self.agent = agent;
        }
        if let Some(eval_timeout) = partial.eval_timeout {
            self.eval_timeout = eval_timeout;
        }
        if let Some(auto_learn) = partial.auto_learn {
            self.auto_learn = auto_learn;
        }
        if let Some(max_iterations) = partial.max_iterations {
            self.max_iterations = max_iterations;
        }
        if let Some(user_message_pause) = partial.user_message_pause {
            self.user_message_pause = user_message_pause;
        }
        if let Some(human_in_the_loop) = partial.human_in_the_loop {
            self.human_in_the_loop = human_in_the_loop;
        }
        if let Some(compaction_enabled) = partial.compaction_enabled {
            self.compaction_enabled = compaction_enabled;
        }
        if let Some(compaction_threshold) = partial.compaction_threshold {
            self.compaction_threshold = compaction_threshold;
        }
        if let Some(compaction_keep_messages) = partial.compaction_keep_messages {
            self.compaction_keep_messages = compaction_keep_messages;
        }
        if let Some(context_warning_threshold) = partial.context_warning_threshold {
            self.context_warning_threshold = context_warning_threshold;
        }
        if let Some(coordinator_port) = partial.coordinator_port {
            self.coordinator_port = coordinator_port;
        }
        if let Some(auth) = partial.auth {
            self.auth = auth;
        }
        if let Some(runners) = partial.runners {
            self.runners = runners;
        }
        if let Some(default_runner) = partial.default_runner {
            self.default_runner = default_runner;
        }
        if let Some(worker_runners) = partial.worker_runners {
            self.worker_runners = worker_runners;
        }
        if let Some(default_profile) = partial.default_profile {
            self.default_profile = default_profile;
        }
        if let Some(profiles) = partial.profiles {
            self.profiles = profiles;
        }
        if let Some(git) = partial.git {
            self.git = git;
        }
        if let Some(storage) = partial.storage {
            self.storage = storage;
        }
        if let Some(allow_local_workers) = partial.allow_local_workers {
            self.allow_local_workers = allow_local_workers;
        }
        if let Some(service_workers) = partial.service_workers {
            self.service_workers = service_workers;
        }
        if let Some(scribe_docs_path) = partial.scribe_docs_path {
            self.scribe_docs_path = scribe_docs_path;
        }
        if let Some(scribe_persist_docs_changes) = partial.scribe_persist_docs_changes {
            self.scribe_persist_docs_changes = scribe_persist_docs_changes;
        }
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

        // Allow disabling local workers via env var (useful for Fly.io deployments)
        if let Ok(val) = env::var("HIRSEL_ALLOW_LOCAL_WORKERS") {
            config.allow_local_workers = val != "0" && val.to_lowercase() != "false";
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

    /// Save the current configuration to config.toml and database
    pub fn save(&self) -> Result<(), ConfigError> {
        // Save to file
        saver::save_config(self, &self.config_file())?;

        // Also save to database
        let store = ConfigStore::open()?;
        store.save_config(self)?;

        Ok(())
    }

    /// Save the current configuration only to the database (no file write)
    pub fn save_to_db(&self) -> Result<(), ConfigError> {
        let store = ConfigStore::open()?;
        store.save_config(self)?;
        Ok(())
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

/// Test utilities for setting up isolated hirsel environments.
///
/// Use `TestEnv` to create a temporary hirsel root with custom config:
///
/// ```ignore
/// use hirsel::core::config::TestEnv;
///
/// let env = TestEnv::new()
///     .with_config(r#"
///         [runners.docker]
///         host = "local"
///         [runners.docker.container]
///         image = "rust:latest"
///     "#)
///     .build();
///
/// // HIRSEL_ROOT is now set to a temp directory
/// let (config, _) = Config::load().unwrap();
/// assert!(config.runners.contains_key("docker"));
/// ```
#[cfg(test)]
pub mod testing {
    use std::fs;
    use tempfile::TempDir;

    /// A test environment with isolated HIRSEL_ROOT.
    pub struct TestEnv {
        #[allow(dead_code)]
        temp_dir: TempDir,
        prev_root: Option<String>,
    }

    impl TestEnv {
        /// Create a new test environment builder.
        pub fn new() -> TestEnvBuilder {
            TestEnvBuilder {
                config_content: None,
            }
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            // Restore previous HIRSEL_ROOT
            match &self.prev_root {
                Some(prev) => std::env::set_var("HIRSEL_ROOT", prev),
                None => std::env::remove_var("HIRSEL_ROOT"),
            }
        }
    }

    /// Builder for TestEnv.
    pub struct TestEnvBuilder {
        config_content: Option<String>,
    }

    impl TestEnvBuilder {
        /// Create a new builder with default settings.
        pub fn new() -> Self {
            Self {
                config_content: None,
            }
        }

        /// Set custom config content (TOML format).
        pub fn with_config(mut self, content: &str) -> Self {
            self.config_content = Some(content.to_string());
            self
        }

        /// Build the test environment.
        ///
        /// This creates a temp directory, writes the config, and sets HIRSEL_ROOT.
        pub fn build(self) -> TestEnv {
            let temp_dir = TempDir::new().expect("Failed to create temp dir");

            // Write config if provided
            if let Some(content) = &self.config_content {
                let config_path = temp_dir.path().join("config.toml");
                fs::write(&config_path, content).expect("Failed to write config");
            }

            // Create runs directory
            let runs_dir = temp_dir.path().join("runs");
            fs::create_dir_all(&runs_dir).expect("Failed to create runs dir");

            // Save previous HIRSEL_ROOT and set new one
            let prev_root = std::env::var("HIRSEL_ROOT").ok();
            std::env::set_var("HIRSEL_ROOT", temp_dir.path());

            TestEnv {
                temp_dir,
                prev_root,
            }
        }
    }

    impl Default for TestEnvBuilder {
        fn default() -> Self {
            Self {
                config_content: None,
            }
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

    #[test]
    fn test_hirsel_root() {
        let _env = testing::TestEnv::new()
            .with_config(r#"eval_timeout = 120"#)
            .build();

        let (config, _) = Config::load().unwrap();
        assert_eq!(config.eval_timeout, 120);
    }
}
