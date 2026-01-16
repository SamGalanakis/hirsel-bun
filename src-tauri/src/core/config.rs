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

/// Regex patterns for worker scale parsing
static WORKER_SCALE_PLUS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)\+$").expect("invalid regex"));
static WORKER_SCALE_RANGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)-(\d+)$").expect("invalid regex"));
static WORKER_SCALE_FIXED_RE: LazyLock<Regex> =
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

    #[error("Invalid workers format: '{value}'. Use: '3' (fixed), '1-5' (range), or '2+' (min with no max)")]
    InvalidWorkerScale { value: String },

    #[error("Minimum workers must be at least 1")]
    MinWorkersTooLow,

    #[error("Maximum workers must be >= minimum")]
    MaxWorkersLessThanMin,

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
        let cmd = command[0].to_lowercase();
        if cmd.contains("claude") {
            Self::Claude
        } else if cmd.contains("gemini") {
            Self::Gemini
        } else if cmd.contains("codex") {
            Self::Codex
        } else if cmd.contains("goose") {
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
    #[default]
    Env,
    ApiKey,
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
/// Parses --workers argument:
/// - "3"   -> fixed 3 workers (min=3, max=3, autoscale=false)
/// - "1-5" -> autoscale between 1 and 5 (min=1, max=5, autoscale=true)
/// - "2+"  -> autoscale from 2 to unlimited (min=2, max=None, autoscale=true)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerScale {
    pub min: u32,
    pub max: Option<u32>,
    pub autoscale: bool,
}

impl WorkerScale {
    /// Parse worker scale from string
    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        let value = value.trim();

        if let Some(caps) = WORKER_SCALE_PLUS_RE.captures(value) {
            let min_val: u32 = caps[1].parse().unwrap();
            if min_val < 1 {
                return Err(ConfigError::MinWorkersTooLow);
            }
            return Ok(Self {
                min: min_val,
                max: None,
                autoscale: true,
            });
        }

        if let Some(caps) = WORKER_SCALE_RANGE_RE.captures(value) {
            let min_val: u32 = caps[1].parse().unwrap();
            let max_val: u32 = caps[2].parse().unwrap();
            if min_val < 1 {
                return Err(ConfigError::MinWorkersTooLow);
            }
            if max_val < min_val {
                return Err(ConfigError::MaxWorkersLessThanMin);
            }
            return Ok(Self {
                min: min_val,
                max: Some(max_val),
                autoscale: true,
            });
        }

        if let Some(caps) = WORKER_SCALE_FIXED_RE.captures(value) {
            let count: u32 = caps[1].parse().unwrap();
            if count < 1 {
                return Err(ConfigError::MinWorkersTooLow);
            }
            return Ok(Self {
                min: count,
                max: Some(count),
                autoscale: false,
            });
        }

        Err(ConfigError::InvalidWorkerScale {
            value: value.to_string(),
        })
    }

    /// Number of workers to start with
    pub fn initial_count(&self) -> u32 {
        self.min
    }

    /// Check if we can add more workers
    pub fn can_scale_up(&self, current: u32) -> bool {
        if !self.autoscale {
            return false;
        }
        match self.max {
            None => true,
            Some(max) => current < max,
        }
    }
}

impl Default for WorkerScale {
    fn default() -> Self {
        Self {
            min: 1,
            max: Some(1),
            autoscale: false,
        }
    }
}

impl std::fmt::Display for WorkerScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.autoscale {
            write!(f, "{}", self.min)
        } else if self.max.is_none() {
            write!(f, "{}+", self.min)
        } else {
            write!(f, "{}-{}", self.min, self.max.unwrap())
        }
    }
}

/// Configuration for a remote machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub host: String,
    #[serde(default = "default_python_path")]
    pub python_path: String,
    #[serde(default = "default_work_base")]
    pub work_base: String,
    pub ssh_key: Option<String>,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub location: Option<String>,
}

fn default_python_path() -> String {
    "python3".to_string()
}

fn default_work_base() -> String {
    "/tmp/hirsel-remote".to_string()
}

fn default_ssh_port() -> u16 {
    22
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_command")]
    pub command: Vec<String>,
}

fn default_agent_command() -> Vec<String> {
    vec!["claude-code-acp".to_string()]
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

    #[serde(default)]
    pub remotes: HashMap<String, RemoteConfig>,

    #[serde(default = "default_coordinator_port")]
    pub coordinator_port: u16,

    pub default_remote: Option<String>,

    #[serde(default)]
    pub auth: AuthConfig,
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
            remotes: HashMap::new(),
            coordinator_port: default_coordinator_port(),
            default_remote: None,
            auth: AuthConfig::default(),
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

        let data: toml::Value =
            content
                .parse()
                .map_err(|e: toml::de::Error| ConfigError::InvalidToml {
                    path: config_path.clone(),
                    message: e.to_string(),
                })?;

        let table = match data.as_table() {
            Some(t) => t,
            None => return Ok(warnings),
        };

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

        // Load remotes
        if let Some(remotes_data) = table.get("remotes") {
            if let Some(remotes_table) = remotes_data.as_table() {
                for (name, remote_data) in remotes_table {
                    if let Some(remote_table) = remote_data.as_table() {
                        if let Some(host) = remote_table.get("host").and_then(|v| v.as_str()) {
                            let remote_config = RemoteConfig {
                                host: host.to_string(),
                                python_path: remote_table
                                    .get("python_path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("python3")
                                    .to_string(),
                                work_base: remote_table
                                    .get("work_base")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("/tmp/hirsel-remote")
                                    .to_string(),
                                ssh_key: remote_table
                                    .get("ssh_key")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                ssh_port: remote_table
                                    .get("ssh_port")
                                    .and_then(|v| v.as_integer())
                                    .unwrap_or(22) as u16,
                                location: remote_table
                                    .get("location")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                            };
                            self.remotes.insert(name.clone(), remote_config);
                        } else {
                            warnings.push(format!(
                                "Config warning: [remotes.{}] missing required 'host' field",
                                name
                            ));
                        }
                    }
                }
            }
        }

        // Load default_remote
        if let Some(val) = table.get("default_remote") {
            if let Some(s) = val.as_str() {
                if s == "local" || s.is_empty() {
                    self.default_remote = None;
                } else {
                    self.default_remote = Some(s.to_string());
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_scale_fixed() {
        let scale = WorkerScale::parse("3").unwrap();
        assert_eq!(scale.min, 3);
        assert_eq!(scale.max, Some(3));
        assert!(!scale.autoscale);
        assert_eq!(scale.to_string(), "3");
    }

    #[test]
    fn test_worker_scale_range() {
        let scale = WorkerScale::parse("1-5").unwrap();
        assert_eq!(scale.min, 1);
        assert_eq!(scale.max, Some(5));
        assert!(scale.autoscale);
        assert_eq!(scale.to_string(), "1-5");
    }

    #[test]
    fn test_worker_scale_unlimited() {
        let scale = WorkerScale::parse("2+").unwrap();
        assert_eq!(scale.min, 2);
        assert_eq!(scale.max, None);
        assert!(scale.autoscale);
        assert_eq!(scale.to_string(), "2+");
    }

    #[test]
    fn test_worker_scale_invalid() {
        assert!(WorkerScale::parse("0").is_err());
        assert!(WorkerScale::parse("abc").is_err());
        assert!(WorkerScale::parse("5-2").is_err());
    }

    #[test]
    fn test_worker_scale_can_scale_up() {
        let fixed = WorkerScale::parse("3").unwrap();
        assert!(!fixed.can_scale_up(3));
        assert!(!fixed.can_scale_up(2));

        let range = WorkerScale::parse("1-5").unwrap();
        assert!(range.can_scale_up(3));
        assert!(!range.can_scale_up(5));

        let unlimited = WorkerScale::parse("2+").unwrap();
        assert!(unlimited.can_scale_up(100));
    }

    #[test]
    fn test_agent_type_from_command() {
        assert_eq!(
            AgentType::from_command(&["claude-code-acp".to_string()]),
            AgentType::Claude
        );
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
