//! Configuration system for hirsel.
//!
//! This module provides the configuration system for hirsel, including:
//! - Agent container configuration
//! - Backend connection configuration
//! - The shared Config struct used by the desktop shell and backend

mod backend;
mod llm;
mod loader;
pub mod paths;
mod saver;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use thiserror::Error;

pub use backend::BackendConfig;
pub use lash::McpServerConfig;
pub use llm::{AgentModelOverrides, LlmConfig, LlmProvider};
pub use paths::{global_db_path, hirsel_dir, project_assets_dir, workspace_dir, workspaces_dir};

/// Context window sizes per model (in tokens).
pub const CONTEXT_WINDOWS: &[(&str, u32)] = &[("gpt-5", 200_000), ("gpt-5-mini", 200_000)];

/// Default context window size for unknown models.
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
    #[error("Invalid TOML in config file {path}: {message}")]
    InvalidToml { path: PathBuf, message: String },

    #[error("Cannot read config file {path}: permission denied")]
    PermissionDenied { path: PathBuf },

    #[error("Failed to read config file {path}: {message}")]
    ReadError { path: PathBuf, message: String },

    #[error("{0}")]
    ValidationError(String),
}

// Default value functions
fn default_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hirsel")
}

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_root")]
    pub root: PathBuf,

    #[serde(default)]
    pub llm: LlmConfig,

    /// Agent container configuration for the backend host.
    #[serde(default)]
    pub sandbox: crate::backend::sandbox::SandboxConfig,

    /// Backend connection used by remote clients.
    #[serde(default)]
    pub backend: BackendConfig,

    /// MCP servers imported into embedded lash sessions.
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: default_root(),
            llm: LlmConfig::default(),
            sandbox: crate::backend::sandbox::SandboxConfig::docker_nix(),
            backend: BackendConfig::default(),
            mcp_servers: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Create a new config from environment and config file.
    ///
    /// Load priority:
    /// 1. Default values
    /// 2. `HIRSEL_ROOT` override (to choose the config location)
    /// 3. Config file (`~/.hirsel/config.toml`)
    ///
    /// Environment variables:
    /// - `HIRSEL_ROOT`: Override the hirsel root directory (default: ~/.hirsel)
    pub fn load() -> Result<(Self, Vec<String>), ConfigError> {
        let mut config = Self::default();
        let mut warnings = Vec::new();

        // Apply env vars for root path first so we read the correct config file.
        if let Ok(root) = env::var("HIRSEL_ROOT") {
            config.root = PathBuf::from(root);
        }

        let config_path = config.config_file();
        if config_path.exists() {
            let file_warnings = loader::load_config_file(&mut config, &config_path)?;
            warnings.extend(file_warnings);
        }

        Ok((config, warnings))
    }

    /// Create config from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(root) = env::var("HIRSEL_ROOT") {
            config.root = PathBuf::from(root);
        }

        config
    }

    /// Path to config file
    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// Save the current configuration to `config.toml`.
    pub fn save(&self) -> Result<(), ConfigError> {
        saver::save_config(self, &self.config_file())
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
///         [sandbox]
///         image = "hirsel-worker:local"
///     "#)
///     .build();
///
/// // HIRSEL_ROOT is now set to a temp directory
/// let (config, _) = Config::load().unwrap();
/// assert_eq!(config.sandbox.image, "hirsel-worker:local");
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

            // Create workspace directory
            let workspaces_dir = temp_dir.path().join("workspaces");
            fs::create_dir_all(&workspaces_dir).expect("Failed to create workspaces dir");

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
    fn test_hirsel_root() {
        let _env = testing::TestEnv::builder()
            .with_config(
                r#"[sandbox]
image = "hirsel-worker:local""#,
            )
            .build();

        let (config, _) = Config::load().unwrap();
        assert_eq!(config.sandbox.image, "hirsel-worker:local");
    }
}
