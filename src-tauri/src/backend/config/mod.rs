//! Configuration system for hirsel.
//!
//! This module provides the configuration system for hirsel, including:
//! - Agent configuration (command, type detection)
//! - Authentication configuration (env vars, API keys, OAuth)
//! - Worker sandbox configuration
//! - Backend connection configuration
//! - Main Config struct with all settings

mod agent;
mod llm;
mod loader;
mod orchestrator;
pub mod paths;
mod saver;
mod storage;
pub mod store;
mod types;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::future::Future;
use std::path::PathBuf;
use thiserror::Error;

/// Block on an async future in a sync context.
/// If already running in an async context, uses the current runtime.
fn block_on<F: Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f)),
        Err(_) => {
            // No runtime, create one
            tokio::runtime::Runtime::new()
                .expect("Failed to create tokio runtime")
                .block_on(f)
        }
    }
}

// Re-export all public types
pub use agent::AgentConfig;
pub use lash::McpServerConfig;
pub use llm::{AgentModelOverrides, LlmConfig, LlmProvider};
pub use orchestrator::BackendConfig;
pub use paths::{
    global_db_path, hirsel_dir, project_assets_dir, runtime_dir, runtime_exists, runtimes_dir,
};
pub use storage::{S3Config, StorageBackend, StorageConfig, StorageProvider};
pub use store::{ConfigStore, ConfigStoreError, PartialConfig};
pub use types::AgentType;

// Re-export model context window constants
pub use super::constants::{CONTEXT_WINDOWS, DEFAULT_CONTEXT_WINDOW};

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
    #[error("No run selected. Set HIRSEL_RUNTIME env var.")]
    NoRunSelected,

    #[error("Invalid TOML in config file {path}: {message}")]
    InvalidToml { path: PathBuf, message: String },

    #[error("Cannot read config file {path}: permission denied")]
    PermissionDenied { path: PathBuf },

    #[error("Failed to read config file {path}: {message}")]
    ReadError { path: PathBuf, message: String },

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

fn default_human_in_the_loop() -> bool {
    true
}

fn default_context_warning_threshold() -> f64 {
    0.5
}

fn default_coordinator_port() -> u16 {
    19700
}

fn default_scribe_batch_window() -> u32 {
    3
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

    #[serde(default = "default_human_in_the_loop")]
    pub human_in_the_loop: bool,

    #[serde(default = "default_context_warning_threshold")]
    pub context_warning_threshold: f64,

    #[serde(default = "default_coordinator_port")]
    pub coordinator_port: u16,

    #[serde(default)]
    pub llm: LlmConfig,

    /// Worker sandbox configuration for the backend host.
    #[serde(default)]
    pub sandbox: crate::backend::runner::RunnerConfig,

    /// Backend connection used by remote clients.
    #[serde(default)]
    pub backend: BackendConfig,

    /// MCP servers imported into embedded lash sessions.
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,

    /// Storage configuration for files and database
    #[serde(default)]
    pub storage: StorageConfig,

    /// How long to wait (in seconds) for more submissions before processing a scribe batch (default: 3)
    #[serde(default = "default_scribe_batch_window")]
    pub scribe_batch_window_seconds: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: default_root(),
            run: None,
            agent: AgentConfig::default(),
            eval_timeout: default_eval_timeout(),
            human_in_the_loop: default_human_in_the_loop(),
            context_warning_threshold: default_context_warning_threshold(),
            coordinator_port: default_coordinator_port(),
            llm: LlmConfig::default(),
            sandbox: crate::backend::runner::RunnerConfig::local(),
            backend: BackendConfig::default(),
            mcp_servers: BTreeMap::new(),
            storage: StorageConfig::default(),
            scribe_batch_window_seconds: default_scribe_batch_window(),
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
    /// - `HIRSEL_RUNTIME`: Set the current run name
    pub fn load() -> Result<(Self, Vec<String>), ConfigError> {
        let mut config = Self::default();
        let mut warnings = Vec::new();

        // Apply env vars for root path first (needed for DB path)
        if let Ok(root) = env::var("HIRSEL_ROOT") {
            config.root = PathBuf::from(root);
        }

        // 1. Try loading from DB
        match block_on(ConfigStore::open()) {
            Ok(store) => {
                if let Some(partial) = block_on(store.load_config())? {
                    config.merge_from(partial);
                }

                // 2. Check for config file override
                let config_path = config.config_file();
                if config_path.exists() {
                    let file_warnings = loader::load_config_file(&mut config, &config_path)?;
                    warnings.extend(file_warnings);

                    // Save file config to DB (one-time migration or update)
                    if let Err(e) = block_on(store.save_config(&config)) {
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
        if let Ok(runtime_name) = env::var("HIRSEL_RUNTIME") {
            config.run = Some(runtime_name);
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
        if let Some(human_in_the_loop) = partial.human_in_the_loop {
            self.human_in_the_loop = human_in_the_loop;
        }
        if let Some(context_warning_threshold) = partial.context_warning_threshold {
            self.context_warning_threshold = context_warning_threshold;
        }
        if let Some(coordinator_port) = partial.coordinator_port {
            self.coordinator_port = coordinator_port;
        }
        if let Some(scribe_batch_window_seconds) = partial.scribe_batch_window_seconds {
            self.scribe_batch_window_seconds = scribe_batch_window_seconds;
        }
        if let Some(llm) = partial.llm {
            self.llm = llm;
        }
        if let Some(sandbox) = partial.sandbox {
            self.sandbox = sandbox;
        }
        if let Some(backend) = partial.backend {
            self.backend = backend;
        }
        if let Some(mcp_servers) = partial.mcp_servers {
            self.mcp_servers = mcp_servers;
        }
        if let Some(storage) = partial.storage {
            self.storage = storage;
        }
    }

    /// Create config from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(root) = env::var("HIRSEL_ROOT") {
            config.root = PathBuf::from(root);
        }

        if let Ok(runtime_name) = env::var("HIRSEL_RUNTIME") {
            config.run = Some(runtime_name);
        }

        config
    }

    /// Path to route runtime workspaces.
    pub fn runtimes_dir(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    /// Path to staging root directory
    pub fn staging_root(&self) -> PathBuf {
        PathBuf::from("/tmp/hirsel-staging")
    }

    /// Path to staging directory for a specific runtime
    pub fn staging_dir(&self, name: &str) -> PathBuf {
        self.staging_root().join(name)
    }

    /// Path to the currently selected runtime directory
    pub fn runtime_dir(&self) -> Result<PathBuf, ConfigError> {
        let run = self.run.as_ref().ok_or(ConfigError::NoRunSelected)?;
        Ok(self.runtimes_dir().join(run))
    }

    /// Path to database file
    pub fn db_path(&self) -> Result<PathBuf, ConfigError> {
        Ok(self.runtime_dir()?.join("hirsel.db"))
    }

    /// Path to work directory
    pub fn work_dir(&self) -> Result<PathBuf, ConfigError> {
        Ok(self.runtime_dir()?.join("work"))
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
    pub fn validate_runtime_name(name: &str) -> Result<(), ConfigError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ConfigError::EmptyRunName);
        }
        if name.contains('/') || name.contains('\\') {
            return Err(ConfigError::RunNameHasPathSeparators);
        }
        Ok(())
    }

    /// Get the backend sandbox configuration used for workers.
    pub fn sandbox_config(&self) -> crate::backend::runner::RunnerConfig {
        self.sandbox.clone()
    }

    /// Save the current configuration to config.toml and database
    pub fn save(&self) -> Result<(), ConfigError> {
        // Save to file
        saver::save_config(self, &self.config_file())?;

        // Also save to database
        let store = block_on(ConfigStore::open())?;
        block_on(store.save_config(self))?;

        Ok(())
    }

    /// Save the current configuration only to the database (no file write)
    pub fn save_to_db(&self) -> Result<(), ConfigError> {
        let store = block_on(ConfigStore::open())?;
        block_on(store.save_config(self))?;
        Ok(())
    }

    /// Update general settings.
    pub fn update_general(
        &mut self,
        eval_timeout: Option<u32>,
        human_in_the_loop: Option<bool>,
        coordinator_port: Option<u16>,
    ) {
        if let Some(v) = eval_timeout {
            self.eval_timeout = v;
        }
        if let Some(v) = human_in_the_loop {
            self.human_in_the_loop = v;
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
}

/// Test utilities for setting up isolated hirsel environments.
///
/// Use `TestEnv` to create a temporary hirsel root with custom config:
///
/// ```ignore
/// use hirsel::core::config::TestEnv;
///
/// let env = TestEnv::builder()
///     .with_config(r#"
///         [sandbox.container]
///         image = "rust:latest"
///     "#)
///     .build();
///
/// // HIRSEL_ROOT is now set to a temp directory
/// let (config, _) = Config::load().unwrap();
/// assert!(config.sandbox.uses_container());
/// ```
#[cfg(test)]
pub mod testing {
    use std::fs;
    use tempfile::TempDir;

    /// A test environment with isolated HIRSEL_ROOT.
    pub struct TestEnv {
        _temp_dir: TempDir,
        prev_root: Option<String>,
    }

    impl TestEnv {
        /// Create a new test environment builder.
        pub fn builder() -> TestEnvBuilder {
            TestEnvBuilder::default()
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
    #[derive(Default)]
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
            let runtimes_dir = temp_dir.path().join("runtimes");
            fs::create_dir_all(&runtimes_dir).expect("Failed to create runs dir");

            // Save previous HIRSEL_ROOT and set new one
            let prev_root = std::env::var("HIRSEL_ROOT").ok();
            std::env::set_var("HIRSEL_ROOT", temp_dir.path());

            TestEnv {
                _temp_dir: temp_dir,
                prev_root,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_runtime_name() {
        assert!(Config::validate_runtime_name("myrun").is_ok());
        assert!(Config::validate_runtime_name("my-run").is_ok());
        assert!(Config::validate_runtime_name("").is_err());
        assert!(Config::validate_runtime_name("my/run").is_err());
        assert!(Config::validate_runtime_name("my\\run").is_err());
    }

    #[test]
    fn test_hirsel_root() {
        let _env = testing::TestEnv::builder()
            .with_config(r#"eval_timeout = 120"#)
            .build();

        let (config, _) = Config::load().unwrap();
        assert_eq!(config.eval_timeout, 120);
    }
}
