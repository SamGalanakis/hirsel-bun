//! Configuration system for hirsel.
//!
//! This module provides the configuration system for hirsel, including:
//! - Agent configuration (command, type detection)
//! - Authentication configuration (env vars, API keys, OAuth)
//! - Worker scaling configuration
//! - Remote worker configuration
//! - Main Config struct with all settings

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use thiserror::Error;

/// Regex pattern for parsing worker scale
static WORKER_SCALE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)$").expect("invalid regex"));

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

/// Type of AI agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Gemini,
    Codex,
    Goose,
    #[default]
    Unknown,
}

impl AgentType {
    /// Detect agent type from command
    pub fn from_command(command: &[String]) -> Self {
        if command.is_empty() {
            return Self::Unknown;
        }
        // Check full command for identification (handles "hirsel __acp-bridge" etc.)
        let full_cmd = command.join(" ").to_lowercase();
        if full_cmd.contains("claude") || full_cmd.contains("__acp-bridge") {
            Self::Claude
        } else if full_cmd.contains("gemini") {
            Self::Gemini
        } else if full_cmd.contains("codex") {
            Self::Codex
        } else if full_cmd.contains("goose") {
            Self::Goose
        } else {
            Self::Unknown
        }
    }

    /// Get the primary environment variable name for this agent type
    pub fn default_env_var(&self) -> Option<&'static str> {
        match self {
            Self::Claude => Some("ANTHROPIC_API_KEY"),
            Self::Gemini => Some("GOOGLE_API_KEY"),
            Self::Codex => Some("OPENAI_API_KEY"),
            Self::Goose => Some("ANTHROPIC_API_KEY"),
            Self::Unknown => None,
        }
    }

    /// Whether this agent type supports context tracking
    pub fn supports_context_tracking(&self) -> bool {
        matches!(self, Self::Claude)
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Gemini => write!(f, "gemini"),
            Self::Codex => write!(f, "codex"),
            Self::Goose => write!(f, "goose"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Authentication method for agents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Env,
    ApiKey,
    #[default]
    OAuth,
}

impl std::str::FromStr for AuthMethod {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "env" => Ok(Self::Env),
            "api_key" => Ok(Self::ApiKey),
            "oauth" => Ok(Self::OAuth),
            _ => Err(ConfigError::ValidationError(format!(
                "Invalid auth method: '{}'. Must be one of: env, api_key, oauth",
                s
            ))),
        }
    }
}

/// Authentication configuration for a specific agent type
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentAuth {
    #[serde(default)]
    pub method: AuthMethod,
    pub api_key: Option<String>,
    pub env_var: Option<String>,
}

impl AgentAuth {
    /// Get credentials as environment variables to forward
    pub fn get_credentials(&self, agent_type: AgentType) -> HashMap<String, String> {
        let mut result = HashMap::new();

        match self.method {
            AuthMethod::ApiKey => {
                if let Some(ref api_key) = self.api_key {
                    let env_var = self
                        .env_var
                        .as_deref()
                        .or_else(|| agent_type.default_env_var());
                    if let Some(var) = env_var {
                        result.insert(var.to_string(), api_key.clone());
                    }
                }
            }
            AuthMethod::Env => {
                let env_var = self
                    .env_var
                    .as_deref()
                    .or_else(|| agent_type.default_env_var());
                if let Some(var) = env_var {
                    if let Ok(value) = env::var(var) {
                        result.insert(var.to_string(), value);
                    }
                }
            }
            AuthMethod::OAuth => {
                if agent_type != AgentType::Claude {
                    return result;
                }
                if let Some(creds) = get_claude_oauth_credentials() {
                    result.extend(creds);
                }
            }
        }

        result
    }
}

/// Read Claude OAuth credentials from ~/.claude/.credentials.json
fn get_claude_oauth_credentials() -> Option<HashMap<String, String>> {
    let credentials_path = dirs::home_dir()?.join(".claude").join(".credentials.json");
    if !credentials_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&credentials_path).ok()?;
    let data: serde_json::Value = serde_json::from_str(&content).ok()?;

    let access_token = data
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()?
        .to_string();

    let mut result = HashMap::new();
    result.insert("CLAUDE_ACCESS_TOKEN".to_string(), access_token);
    Some(result)
}

/// Authentication configuration for all agent types
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    pub claude: Option<AgentAuth>,
    pub gemini: Option<AgentAuth>,
    pub codex: Option<AgentAuth>,
    pub goose: Option<AgentAuth>,
    #[serde(default)]
    pub default_method: AuthMethod,
}

impl AuthConfig {
    /// Get auth config for a specific agent type
    pub fn get_auth_for(&self, agent_type: AgentType) -> AgentAuth {
        match agent_type {
            AgentType::Claude => self.claude.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Gemini => self.gemini.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Codex => self.codex.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Goose => self.goose.clone().unwrap_or(AgentAuth {
                method: self.default_method,
                ..Default::default()
            }),
            AgentType::Unknown => AgentAuth {
                method: self.default_method,
                ..Default::default()
            },
        }
    }
}

/// Get credentials for an agent type as environment variables
pub fn get_agent_env_vars(
    agent_type: AgentType,
    auth_config: &AuthConfig,
) -> HashMap<String, String> {
    let agent_auth = auth_config.get_auth_for(agent_type);
    agent_auth.get_credentials(agent_type)
}

/// Worker scaling configuration.
///
/// Workers is just a max count - always starts with 1 and autoscales up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerScale {
    pub max: u32,
}

impl WorkerScale {
    /// Parse worker scale from string - just the max worker count.
    /// - "4" -> autoscale up to 4 workers
    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        let value = value.trim();

        // Simple number = max workers
        if let Some(caps) = WORKER_SCALE_RE.captures(value) {
            let max: u32 = caps[1].parse().unwrap();
            if max < 1 {
                return Err(ConfigError::WorkerCountTooLow);
            }
            return Ok(Self { max });
        }

        Err(ConfigError::InvalidWorkerScale {
            value: value.to_string(),
        })
    }

    /// Number of workers to start with - always 1, we autoscale from there
    pub fn initial_count(&self) -> u32 {
        1
    }

    /// Check if we can add more workers
    pub fn can_scale_up(&self, current: u32) -> bool {
        current < self.max
    }
}

impl Default for WorkerScale {
    fn default() -> Self {
        Self { max: 1 }
    }
}

impl std::fmt::Display for WorkerScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.max)
    }
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_command")]
    pub command: Vec<String>,
}

fn default_agent_command() -> Vec<String> {
    // Use the hirsel ACP bridge which wraps the claude CLI
    // This provides ACP protocol support for Claude
    vec!["hirsel".to_string(), "__acp-bridge".to_string()]
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            command: default_agent_command(),
        }
    }
}

impl AgentConfig {
    /// Get the agent type from the command
    pub fn agent_type(&self) -> AgentType {
        AgentType::from_command(&self.command)
    }

    /// Whether this agent supports context tracking
    pub fn supports_context_tracking(&self) -> bool {
        self.agent_type().supports_context_tracking()
    }
}

/// Orchestrator mode - local or remote
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OrchestratorMode {
    #[default]
    Local,
    Remote,
}

/// How workers access the orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OrchestratorAccess {
    /// Direct access - assumes network is already configured (VPC, same network, etc.)
    Direct,
    /// Tailscale - workers join the user's tailnet via OAuth-generated auth keys
    Tailscale {
        /// OAuth client ID from Tailscale admin console
        oauth_client_id: String,
        /// OAuth client secret from Tailscale admin console
        oauth_client_secret: String,
        /// Optional tag to apply to worker devices (e.g., "tag:hirsel-worker")
        #[serde(default)]
        tag: Option<String>,
    },
}

impl Default for OrchestratorAccess {
    fn default() -> Self {
        Self::Direct
    }
}

/// Orchestrator profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorProfile {
    #[serde(default)]
    pub mode: OrchestratorMode,
    /// Server URL for remote mode
    pub url: Option<String>,
    /// API key for remote mode
    pub api_key: Option<String>,
    /// How workers access the orchestrator (network strategy)
    #[serde(default)]
    pub access: OrchestratorAccess,
}

impl Default for OrchestratorProfile {
    fn default() -> Self {
        Self {
            mode: OrchestratorMode::Local,
            url: None,
            api_key: None,
            access: OrchestratorAccess::Direct,
        }
    }
}

impl OrchestratorProfile {
    /// Get Tailscale OAuth credentials if access is configured for Tailscale
    pub fn tailscale_oauth(&self) -> Option<(&str, &str, Option<&str>)> {
        match &self.access {
            OrchestratorAccess::Tailscale {
                oauth_client_id,
                oauth_client_secret,
                tag,
            } => Some((
                oauth_client_id.as_str(),
                oauth_client_secret.as_str(),
                tag.as_deref(),
            )),
            _ => None,
        }
    }
}

/// Git provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitProvider {
    Github,
    // Future: Gitlab, Bitbucket, etc.
}

impl std::fmt::Display for GitProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Github => write!(f, "github"),
        }
    }
}

/// Git provider configuration
///
/// Tokens are stored in CredentialStore, not in config.
/// This struct only tracks which providers are configured.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitConfig {
    /// Default git provider to use
    pub default_provider: Option<GitProvider>,
}

// =============================================================================
// Storage Configuration
// =============================================================================

/// Storage backend type for file storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    /// Local filesystem storage (default)
    #[default]
    Local,
    /// S3-compatible object storage (MinIO, Tigris, AWS S3)
    S3,
}

/// S3-compatible storage configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct S3Config {
    /// S3 endpoint URL (e.g., "http://localhost:9000" for MinIO, or Tigris URL)
    /// If not set, uses AWS S3 default endpoint
    pub endpoint: Option<String>,
    /// S3 bucket name
    #[serde(default)]
    pub bucket: String,
    /// AWS region (e.g., "us-east-1", "auto" for MinIO)
    #[serde(default)]
    pub region: Option<String>,
    /// AWS access key ID (can also be set via environment)
    pub access_key_id: Option<String>,
    /// AWS secret access key (can also be set via environment)
    pub secret_access_key: Option<String>,
}

/// Storage configuration for files and database
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    /// File storage backend: "local" or "s3"
    #[serde(default)]
    pub files: StorageBackend,
    /// S3 configuration (when files = "s3")
    #[serde(default)]
    pub s3: Option<S3Config>,
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

fn default_profile() -> String {
    "local".to_string()
}

fn default_profiles() -> HashMap<String, OrchestratorProfile> {
    let mut profiles = HashMap::new();
    profiles.insert("local".to_string(), OrchestratorProfile::default());
    profiles
}

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
        let warnings = config.load_config_file()?;
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

    /// Load settings from config file
    pub fn load_config_file(&mut self) -> Result<Vec<String>, ConfigError> {
        let mut warnings = Vec::new();
        let config_path = self.config_file();

        if !config_path.exists() {
            return Ok(warnings);
        }

        let content = fs::read_to_string(&config_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ConfigError::PermissionDenied {
                    path: config_path.clone(),
                }
            } else {
                ConfigError::ReadError {
                    path: config_path.clone(),
                    message: e.to_string(),
                }
            }
        })?;

        let table: toml::Table =
            content
                .parse()
                .map_err(|e: toml::de::Error| ConfigError::InvalidToml {
                    path: config_path.clone(),
                    message: e.to_string(),
                })?;

        // Load agent config
        if let Some(agent_data) = table.get("agent") {
            if let Some(agent_table) = agent_data.as_table() {
                if let Some(cmd) = agent_table.get("command") {
                    if let Some(arr) = cmd.as_array() {
                        let command: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                        if !command.is_empty() {
                            self.agent.command = command;
                        }
                    } else {
                        warnings.push(format!(
                            "Config warning: agent.command should be a list of strings, got {}",
                            cmd.type_str()
                        ));
                    }
                }
            } else {
                warnings.push(format!(
                    "Config warning: [agent] section should be a table, got {}",
                    agent_data.type_str()
                ));
            }
        }

        // Load eval_timeout
        if let Some(val) = table.get("eval_timeout") {
            if let Some(timeout) = val.as_integer() {
                let timeout = timeout as u32;
                if timeout < 60 {
                    warnings.push(format!(
                        "Config warning: eval_timeout={} is below minimum (60s), using 60s",
                        timeout
                    ));
                    self.eval_timeout = 60;
                } else if timeout > 7200 {
                    warnings.push(format!(
                        "Config warning: eval_timeout={} exceeds maximum (7200s), using 7200s",
                        timeout
                    ));
                    self.eval_timeout = 7200;
                } else {
                    self.eval_timeout = timeout;
                }
            } else {
                warnings.push(format!(
                    "Config warning: invalid eval_timeout value: {}",
                    val
                ));
            }
        }

        // Load auto_learn
        if let Some(val) = table.get("auto_learn") {
            if let Some(b) = val.as_bool() {
                self.auto_learn = b;
            } else {
                warnings.push(format!(
                    "Config warning: auto_learn should be a boolean, got {}",
                    val.type_str()
                ));
            }
        }

        // Load human_in_the_loop
        if let Some(val) = table.get("human_in_the_loop") {
            if let Some(b) = val.as_bool() {
                self.human_in_the_loop = b;
            } else {
                warnings.push(format!(
                    "Config warning: human_in_the_loop should be a boolean, got {}",
                    val.type_str()
                ));
            }
        }

        // Load user_message_pause
        if let Some(val) = table.get("user_message_pause") {
            if let Some(s) = val.as_str() {
                if s == "sender" || s == "all" {
                    self.user_message_pause = s.to_string();
                } else {
                    warnings.push(format!(
                        "Config warning: user_message_pause must be 'sender' or 'all', got {}",
                        s
                    ));
                }
            }
        }

        // Load coordinator_port
        if let Some(val) = table.get("coordinator_port") {
            if let Some(n) = val.as_integer() {
                if n > 0 {
                    self.coordinator_port = n as u16;
                } else {
                    warnings.push(format!(
                        "Config warning: coordinator_port must be a positive integer, got {}",
                        n
                    ));
                }
            }
        }

        // Load context_warning_threshold
        if let Some(val) = table.get("context_warning_threshold") {
            if let Some(n) = val.as_float() {
                if (0.0..=1.0).contains(&n) {
                    self.context_warning_threshold = n;
                } else {
                    warnings.push(format!(
                        "Config warning: context_warning_threshold must be between 0.0 and 1.0, got {}",
                        n
                    ));
                }
            } else if let Some(n) = val.as_integer() {
                let n = n as f64;
                if (0.0..=1.0).contains(&n) {
                    self.context_warning_threshold = n;
                }
            }
        }

        // Load auth configuration
        if let Some(auth_data) = table.get("auth") {
            if let Some(auth_table) = auth_data.as_table() {
                if let Some(val) = auth_table.get("default_method") {
                    if let Some(s) = val.as_str() {
                        if let Ok(method) = s.parse::<AuthMethod>() {
                            self.auth.default_method = method;
                        }
                    }
                }

                for agent_name in &["claude", "gemini", "codex", "goose"] {
                    if let Some(agent_auth_data) = auth_table.get(*agent_name) {
                        if let Some(agent_auth_table) = agent_auth_data.as_table() {
                            let method_str = agent_auth_table
                                .get("method")
                                .and_then(|v| v.as_str())
                                .unwrap_or("env");

                            if let Ok(method) = method_str.parse::<AuthMethod>() {
                                let agent_auth = AgentAuth {
                                    method,
                                    api_key: agent_auth_table
                                        .get("api_key")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    env_var: agent_auth_table
                                        .get("env_var")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                };
                                match *agent_name {
                                    "claude" => self.auth.claude = Some(agent_auth),
                                    "gemini" => self.auth.gemini = Some(agent_auth),
                                    "codex" => self.auth.codex = Some(agent_auth),
                                    "goose" => self.auth.goose = Some(agent_auth),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        // Load runners configuration
        if let Some(runners_data) = table.get("runners") {
            if let Some(runners_table) = runners_data.as_table() {
                for (name, runner_data) in runners_table {
                    // Serialize TOML value to string, then parse as RunnerConfig
                    let toml_str = toml::to_string(runner_data).unwrap_or_default();
                    match toml::from_str::<crate::core::runner::RunnerConfig>(&toml_str) {
                        Ok(runner_config) => {
                            self.runners.insert(name.clone(), runner_config);
                        }
                        Err(e) => {
                            warnings.push(format!(
                                "Config warning: [runners.{}] invalid config: {}",
                                name, e
                            ));
                        }
                    }
                }
            }
        }

        // Load default_runner
        if let Some(val) = table.get("default_runner") {
            if let Some(s) = val.as_str() {
                if s == "local" || s.is_empty() {
                    self.default_runner = None;
                } else {
                    self.default_runner = Some(s.to_string());
                }
            }
        }

        // Load worker_runners assignments
        if let Some(worker_runners_data) = table.get("worker_runners") {
            if let Some(worker_runners_table) = worker_runners_data.as_table() {
                for (worker_name, runner_name_val) in worker_runners_table {
                    if let Some(runner_name) = runner_name_val.as_str() {
                        self.worker_runners
                            .insert(worker_name.clone(), runner_name.to_string());
                    }
                }
            }
        }

        // Load default_profile
        if let Some(val) = table.get("default_profile") {
            if let Some(s) = val.as_str() {
                self.default_profile = s.to_string();
            }
        }

        // Load orchestrator profiles
        if let Some(profiles_data) = table.get("profiles") {
            if let Some(profiles_table) = profiles_data.as_table() {
                for (name, profile_data) in profiles_table {
                    if let Some(profile_table) = profile_data.as_table() {
                        let mode_str = profile_table
                            .get("mode")
                            .and_then(|v| v.as_str())
                            .unwrap_or("local");

                        let mode = match mode_str {
                            "remote" => OrchestratorMode::Remote,
                            _ => OrchestratorMode::Local,
                        };

                        // Parse access strategy
                        let access = if let Some(access_data) = profile_table.get("access") {
                            if let Some(access_table) = access_data.as_table() {
                                let access_type = access_table
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("direct");

                                match access_type {
                                    "tailscale" => {
                                        let client_id = access_table
                                            .get("oauth_client_id")
                                            .and_then(|v| v.as_str());
                                        let client_secret = access_table
                                            .get("oauth_client_secret")
                                            .and_then(|v| v.as_str());
                                        let tag = access_table
                                            .get("tag")
                                            .and_then(|v| v.as_str())
                                            .map(String::from);

                                        if let (Some(id), Some(secret)) = (client_id, client_secret)
                                        {
                                            OrchestratorAccess::Tailscale {
                                                oauth_client_id: id.to_string(),
                                                oauth_client_secret: secret.to_string(),
                                                tag,
                                            }
                                        } else {
                                            warnings.push(format!(
                                                "Config warning: [profiles.{}.access] tailscale requires 'oauth_client_id' and 'oauth_client_secret'",
                                                name
                                            ));
                                            OrchestratorAccess::Direct
                                        }
                                    }
                                    _ => OrchestratorAccess::Direct,
                                }
                            } else {
                                OrchestratorAccess::Direct
                            }
                        } else {
                            OrchestratorAccess::Direct
                        };

                        let profile = OrchestratorProfile {
                            mode,
                            url: profile_table
                                .get("url")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            api_key: profile_table
                                .get("api_key")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            access,
                        };

                        // Validate remote profiles have required fields
                        if mode == OrchestratorMode::Remote {
                            if profile.url.is_none() {
                                warnings.push(format!(
                                    "Config warning: [profiles.{}] remote mode requires 'url' field",
                                    name
                                ));
                                continue;
                            }
                            // Note: api_key is optional in config - it can be loaded from credential store
                        }

                        self.profiles.insert(name.clone(), profile);
                    }
                }
            }
        }

        // Load git configuration
        if let Some(git_data) = table.get("git") {
            if let Some(git_table) = git_data.as_table() {
                if let Some(provider_str) =
                    git_table.get("default_provider").and_then(|v| v.as_str())
                {
                    self.git.default_provider = match provider_str.to_lowercase().as_str() {
                        "github" => Some(GitProvider::Github),
                        _ => {
                            warnings.push(format!(
                                "Config warning: unknown git provider '{}', ignoring",
                                provider_str
                            ));
                            None
                        }
                    };
                }
            }
        }

        // Load storage configuration
        if let Some(storage_data) = table.get("storage") {
            if let Some(storage_table) = storage_data.as_table() {
                // Parse files backend
                if let Some(files_str) = storage_table.get("files").and_then(|v| v.as_str()) {
                    self.storage.files = match files_str.to_lowercase().as_str() {
                        "local" => StorageBackend::Local,
                        "s3" => StorageBackend::S3,
                        _ => {
                            warnings.push(format!(
                                "Config warning: unknown storage backend '{}', using local",
                                files_str
                            ));
                            StorageBackend::Local
                        }
                    };
                }

                // Parse S3 config if present
                if let Some(s3_data) = storage_table.get("s3") {
                    if let Some(s3_table) = s3_data.as_table() {
                        self.storage.s3 = Some(S3Config {
                            endpoint: s3_table
                                .get("endpoint")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            bucket: s3_table
                                .get("bucket")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            region: s3_table
                                .get("region")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            access_key_id: s3_table
                                .get("access_key_id")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            secret_access_key: s3_table
                                .get("secret_access_key")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                        });
                    }
                }
            }
        }

        Ok(warnings)
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
    ///
    /// This serializes the config back to TOML format and writes it to disk.
    /// Note: Comments in the original file will be lost.
    pub fn save(&self) -> Result<(), ConfigError> {
        let config_path = self.config_file();

        // Build TOML manually to control section ordering
        let mut output = String::new();

        // Agent section
        output.push_str("[agent]\n");
        let cmd_parts: Vec<String> = self
            .agent
            .command
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect();
        output.push_str(&format!("command = [{}]\n\n", cmd_parts.join(", ")));

        // Top-level settings
        output.push_str(&format!("eval_timeout = {}\n", self.eval_timeout));
        output.push_str(&format!("auto_learn = {}\n", self.auto_learn));
        if let Some(max_iter) = self.max_iterations {
            output.push_str(&format!("max_iterations = {}\n", max_iter));
        }
        output.push_str(&format!(
            "user_message_pause = \"{}\"\n",
            self.user_message_pause
        ));
        output.push_str(&format!("human_in_the_loop = {}\n", self.human_in_the_loop));
        output.push_str(&format!(
            "compaction_enabled = {}\n",
            self.compaction_enabled
        ));
        if let Some(threshold) = self.compaction_threshold {
            output.push_str(&format!("compaction_threshold = {}\n", threshold));
        }
        output.push_str(&format!(
            "compaction_keep_messages = {}\n",
            self.compaction_keep_messages
        ));
        output.push_str(&format!("auto_improve = {}\n", self.auto_improve));
        output.push_str(&format!(
            "context_warning_threshold = {}\n",
            self.context_warning_threshold
        ));
        output.push_str(&format!("coordinator_port = {}\n", self.coordinator_port));
        if let Some(ref runner) = self.default_runner {
            output.push_str(&format!("default_runner = \"{}\"\n", runner));
        }
        output.push_str(&format!("default_profile = \"{}\"\n", self.default_profile));
        output.push('\n');

        // Auth section
        output.push_str("[auth]\n");
        output.push_str(&format!(
            "default_method = \"{}\"\n",
            match self.auth.default_method {
                AuthMethod::Env => "env",
                AuthMethod::ApiKey => "api_key",
                AuthMethod::OAuth => "oauth",
            }
        ));

        // Agent-specific auth
        for (name, auth) in [
            ("claude", &self.auth.claude),
            ("gemini", &self.auth.gemini),
            ("codex", &self.auth.codex),
            ("goose", &self.auth.goose),
        ] {
            if let Some(agent_auth) = auth {
                output.push_str(&format!("\n[auth.{}]\n", name));
                output.push_str(&format!(
                    "method = \"{}\"\n",
                    match agent_auth.method {
                        AuthMethod::Env => "env",
                        AuthMethod::ApiKey => "api_key",
                        AuthMethod::OAuth => "oauth",
                    }
                ));
                if let Some(ref key) = agent_auth.api_key {
                    output.push_str(&format!("api_key = \"{}\"\n", key));
                }
                if let Some(ref var) = agent_auth.env_var {
                    output.push_str(&format!("env_var = \"{}\"\n", var));
                }
            }
        }
        output.push('\n');

        // Runners section - using new Host + Container model
        for (name, runner_config) in &self.runners {
            use crate::core::runner::{HostConfig, HostConfigOrShortcut};

            output.push_str(&format!("[runners.{}]\n", name));

            // Serialize host configuration
            let host = runner_config.host.resolve();
            match &runner_config.host {
                HostConfigOrShortcut::Shortcut(s) => {
                    output.push_str(&format!("host = \"{}\"\n", s));
                }
                HostConfigOrShortcut::Full(_) => {
                    // Full host config needs a sub-table
                    match &host {
                        HostConfig::Local => {
                            output.push_str(&format!("\n[runners.{}.host]\n", name));
                            output.push_str("type = \"local\"\n");
                        }
                        HostConfig::Client => {
                            output.push_str(&format!("\n[runners.{}.host]\n", name));
                            output.push_str("type = \"client\"\n");
                        }
                        HostConfig::Ssh(ssh) => {
                            output.push_str(&format!("\n[runners.{}.host]\n", name));
                            output.push_str("type = \"ssh\"\n");
                            output.push_str(&format!("address = \"{}\"\n", ssh.address));
                            if ssh.port != 22 {
                                output.push_str(&format!("port = {}\n", ssh.port));
                            }
                            if let Some(ref key) = ssh.ssh_key {
                                output.push_str(&format!("ssh_key = \"{}\"\n", key));
                            }
                            output.push_str(&format!("work_base = \"{}\"\n", ssh.work_base));
                            if let Some(ref loc) = ssh.location {
                                output.push_str(&format!("location = \"{}\"\n", loc));
                            }
                        }
                        HostConfig::Sprite(sprite) => {
                            output.push_str(&format!("\n[runners.{}.host]\n", name));
                            output.push_str("type = \"sprite\"\n");
                            if let Some(ref token) = sprite.api_token {
                                output.push_str(&format!("api_token = \"{}\"\n", token));
                            }
                            if let Some(ref cp) = sprite.checkpoint {
                                output.push_str(&format!("checkpoint = \"{}\"\n", cp));
                            }
                            output.push_str(&format!("auto_destroy = {}\n", sprite.auto_destroy));
                            output.push_str(&format!(
                                "idle_timeout_secs = {}\n",
                                sprite.idle_timeout_secs
                            ));
                            if sprite.api_url != "https://api.sprites.dev" {
                                output.push_str(&format!("api_url = \"{}\"\n", sprite.api_url));
                            }
                            if sprite.use_file_push {
                                output.push_str("use_file_push = true\n");
                            }
                        }
                        HostConfig::Fly(fly) => {
                            output.push_str(&format!("\n[runners.{}.host]\n", name));
                            output.push_str("type = \"fly\"\n");
                            if let Some(ref token) = fly.api_token {
                                output.push_str(&format!("api_token = \"{}\"\n", token));
                            }
                            output.push_str(&format!("app = \"{}\"\n", fly.app));
                            if let Some(ref region) = fly.region {
                                output.push_str(&format!("region = \"{}\"\n", region));
                            }
                            if fly.cpu_kind != "shared" {
                                output.push_str(&format!("cpu_kind = \"{}\"\n", fly.cpu_kind));
                            }
                            if fly.cpus != 1 {
                                output.push_str(&format!("cpus = {}\n", fly.cpus));
                            }
                            if fly.memory_mb != 1024 {
                                output.push_str(&format!("memory_mb = {}\n", fly.memory_mb));
                            }
                            output.push_str(&format!("auto_destroy = {}\n", fly.auto_destroy));
                        }
                    }
                }
            }

            // Serialize container configuration if present
            if let Some(ref container) = runner_config.container {
                output.push_str(&format!("\n[runners.{}.container]\n", name));
                output.push_str(&format!("image = \"{}\"\n", container.image));
            }

            output.push('\n');
        }

        // Profiles section
        for (name, profile) in &self.profiles {
            output.push_str(&format!("[profiles.{}]\n", name));
            output.push_str(&format!(
                "mode = \"{}\"\n",
                match profile.mode {
                    OrchestratorMode::Local => "local",
                    OrchestratorMode::Remote => "remote",
                }
            ));
            if let Some(ref url) = profile.url {
                output.push_str(&format!("url = \"{}\"\n", url));
            }
            if let Some(ref key) = profile.api_key {
                output.push_str(&format!("api_key = \"{}\"\n", key));
            }

            // Access sub-section
            match &profile.access {
                OrchestratorAccess::Direct => {
                    output.push_str("\n[profiles.");
                    output.push_str(name);
                    output.push_str(".access]\n");
                    output.push_str("type = \"direct\"\n");
                }
                OrchestratorAccess::Tailscale {
                    oauth_client_id,
                    oauth_client_secret,
                    tag,
                } => {
                    output.push_str("\n[profiles.");
                    output.push_str(name);
                    output.push_str(".access]\n");
                    output.push_str("type = \"tailscale\"\n");
                    output.push_str(&format!("oauth_client_id = \"{}\"\n", oauth_client_id));
                    output.push_str(&format!(
                        "oauth_client_secret = \"{}\"\n",
                        oauth_client_secret
                    ));
                    if let Some(ref t) = tag {
                        output.push_str(&format!("tag = \"{}\"\n", t));
                    }
                }
            }
            output.push('\n');
        }

        // Git section
        if let Some(ref provider) = self.git.default_provider {
            output.push_str("[git]\n");
            output.push_str(&format!(
                "default_provider = \"{}\"\n",
                match provider {
                    GitProvider::Github => "github",
                }
            ));
            output.push('\n');
        }

        // Storage section (only write if non-default)
        if self.storage.files != StorageBackend::Local || self.storage.s3.is_some() {
            output.push_str("[storage]\n");
            output.push_str(&format!(
                "files = \"{}\"\n",
                match self.storage.files {
                    StorageBackend::Local => "local",
                    StorageBackend::S3 => "s3",
                }
            ));

            if let Some(ref s3) = self.storage.s3 {
                output.push_str("\n[storage.s3]\n");
                if let Some(ref endpoint) = s3.endpoint {
                    output.push_str(&format!("endpoint = \"{}\"\n", endpoint));
                }
                output.push_str(&format!("bucket = \"{}\"\n", s3.bucket));
                if let Some(ref region) = s3.region {
                    output.push_str(&format!("region = \"{}\"\n", region));
                }
                if let Some(ref key) = s3.access_key_id {
                    output.push_str(&format!("access_key_id = \"{}\"\n", key));
                }
                if let Some(ref secret) = s3.secret_access_key {
                    output.push_str(&format!("secret_access_key = \"{}\"\n", secret));
                }
            }
            output.push('\n');
        }

        // Ensure parent directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).map_err(|e| ConfigError::ReadError {
                path: parent.to_path_buf(),
                message: e.to_string(),
            })?;
        }

        // Write config file
        fs::write(&config_path, output).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ConfigError::PermissionDenied { path: config_path }
            } else {
                ConfigError::ReadError {
                    path: config_path,
                    message: e.to_string(),
                }
            }
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_scale_simple() {
        let scale = WorkerScale::parse("3").unwrap();
        assert_eq!(scale.max, 3);
        assert_eq!(scale.initial_count(), 1);
        assert_eq!(scale.to_string(), "3");
    }

    #[test]
    fn test_worker_scale_invalid() {
        assert!(WorkerScale::parse("0").is_err());
        assert!(WorkerScale::parse("abc").is_err());
        assert!(WorkerScale::parse("1-5").is_err()); // Legacy format not supported
        assert!(WorkerScale::parse("2+").is_err()); // Legacy format not supported
    }

    #[test]
    fn test_worker_scale_can_scale_up() {
        let scale = WorkerScale::parse("5").unwrap();
        assert!(scale.can_scale_up(3));
        assert!(scale.can_scale_up(4));
        assert!(!scale.can_scale_up(5));
        assert!(!scale.can_scale_up(6));
    }

    #[test]
    fn test_agent_type_from_command() {
        // Claude variants
        assert_eq!(
            AgentType::from_command(&["claude-code-acp".to_string()]),
            AgentType::Claude
        );
        assert_eq!(
            AgentType::from_command(&["hirsel".to_string(), "__acp-bridge".to_string()]),
            AgentType::Claude
        );
        // Other agents
        assert_eq!(
            AgentType::from_command(&["gemini".to_string()]),
            AgentType::Gemini
        );
        assert_eq!(
            AgentType::from_command(&["codex".to_string()]),
            AgentType::Codex
        );
        assert_eq!(
            AgentType::from_command(&["goose".to_string()]),
            AgentType::Goose
        );
        assert_eq!(AgentType::from_command(&[]), AgentType::Unknown);
    }

    #[test]
    fn test_validate_run_name() {
        assert!(Config::validate_run_name("myrun").is_ok());
        assert!(Config::validate_run_name("my-run").is_ok());
        assert!(Config::validate_run_name("").is_err());
        assert!(Config::validate_run_name("my/run").is_err());
        assert!(Config::validate_run_name("my\\run").is_err());
    }
}

// =============================================================================
// Convenience Path Functions
// =============================================================================
// Simple standalone functions for quick path access without loading full config

/// Get the hirsel home directory (~/.hirsel)
pub fn hirsel_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".hirsel")
}

/// Get the runs directory (~/.hirsel/runs)
pub fn runs_dir() -> PathBuf {
    hirsel_dir().join("runs")
}

/// Get the path to a specific run
pub fn run_dir(run_name: &str) -> PathBuf {
    runs_dir().join(run_name)
}

/// Get all run names by listing the runs directory
pub fn list_runs() -> std::io::Result<Vec<String>> {
    let runs = runs_dir();
    if !runs.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in std::fs::read_dir(runs)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                // Skip hidden directories
                if !name.starts_with('.') {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Check if a run exists
pub fn run_exists(run_name: &str) -> bool {
    run_dir(run_name).exists()
}

/// Get the path to the global hirsel database (~/.hirsel/hirsel.db)
/// This stores global data like Gyp chat history that shouldn't be in run DBs
pub fn global_db_path() -> PathBuf {
    hirsel_dir().join("hirsel.db")
}
