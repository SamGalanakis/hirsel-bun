//! Tauri IPC commands for the Hirsel GUI
//!
//! These commands provide the interface between the frontend and backend.
//! Types are designed to match the TypeScript definitions in src/lib/types.ts.

use crate::core::{config, metrics, state::SQLiteState};
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Parse a timestamp string and return a DateTime<Utc>
fn parse_timestamp(timestamp: &str) -> Option<chrono::DateTime<Utc>> {
    // Try RFC3339 first (has timezone)
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(timestamp) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try parsing as NaiveDateTime (no timezone, assume UTC)
    // Format: "2024-01-13T12:30:45.123456"
    if let Ok(naive) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(Utc.from_utc_datetime(&naive));
    }

    // Try without fractional seconds
    if let Ok(naive) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }

    None
}

/// Parse a timestamp string (with or without timezone) and return elapsed minutes from now
fn parse_elapsed_minutes(timestamp: &str) -> f64 {
    if let Some(dt) = parse_timestamp(timestamp) {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(dt);
        return elapsed.num_seconds() as f64 / 60.0;
    }
    0.0
}

/// Calculate duration in minutes between two timestamps
fn calculate_duration_minutes(start: &str, end: &str) -> f64 {
    if let (Some(start_dt), Some(end_dt)) = (parse_timestamp(start), parse_timestamp(end)) {
        let duration = end_dt.signed_duration_since(start_dt);
        return (duration.num_seconds() as f64 / 60.0).max(0.0);
    }
    0.0
}

/// Check if a status represents a completed run
fn is_completed_status(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Done
            | RunStatus::Delivered
            | RunStatus::Merged
            | RunStatus::TimedOut
            | RunStatus::EvalFailed
            | RunStatus::Runaway
    )
}

/// Internal cooldown for compaction checks (10 seconds)
const COMPACTION_INTERNAL_COOLDOWN_SECONDS: i64 = 10;

/// Trigger automatic learnings compaction if needed.
/// This is called from get_run_detail polling. It spawns a subprocess
/// to handle compaction (like improve does) to avoid Send issues with ACP.
fn trigger_compaction_if_needed(run_name: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let run_dir = config::run_dir(run_name);
    let files = crate::core::Files::new(&run_dir);
    let state =
        SQLiteState::new(files.db_path()).map_err(|e| format!("Failed to open database: {}", e))?;

    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Quick checks before spawning subprocess
    if !global_config.compaction_enabled {
        return Ok(());
    }

    // Internal cooldown check (10 seconds) - just to prevent rapid-fire triggers
    if let Ok(Some(last_compaction)) = state.get_last_compaction_at() {
        if let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(&last_compaction) {
            let now = Utc::now();
            let elapsed_seconds = (now - last_time.with_timezone(&Utc)).num_seconds();
            if elapsed_seconds < COMPACTION_INTERNAL_COOLDOWN_SECONDS {
                tracing::warn!(
                    "Compaction triggered within {}s of last compaction ({}s ago) for run '{}'",
                    COMPACTION_INTERNAL_COOLDOWN_SECONDS,
                    elapsed_seconds,
                    run_name
                );
                return Ok(()); // Internal cooldown not elapsed
            }
        }
    }

    // Check if compaction is actually needed (threshold check)
    let check_result =
        crate::core::compaction::check_learnings_compaction_with_config(&state, &global_config);
    if !matches!(check_result, Ok(Some(_))) {
        return Ok(()); // Not needed
    }

    // Spawn compaction subprocess
    let hirsel_exe =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;

    let mut cmd = Command::new(&hirsel_exe);
    cmd.arg("__compact-learnings")
        .arg(run_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Spawn detached
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    match cmd.spawn() {
        Ok(child) => {
            tracing::info!(
                "Spawned compaction process for '{}', pid={}",
                run_name,
                child.id()
            );
            Ok(())
        }
        Err(e) => Err(format!("Failed to spawn compaction: {}", e)),
    }
}

// =============================================================================
// Status Enums (match TypeScript types)
// =============================================================================

/// Run status values matching TypeScript RunStatus
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Draft,
    Idle,
    Working,
    Paused,
    Runaway,
    TimedOut,
    Eval,
    EvalFailed,
    Waiting,
    Done,
    Delivered,
    Merged,
}

/// Task status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
}

/// Worker status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Idle,
    Working,
    Waiting,
    Awaiting,
    Paused,
    Error,
}

/// Worker location
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerLocation {
    Local,
    Remote,
}

/// Eval status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalStatus {
    Running,
    Passed,
    Failed,
}

// =============================================================================
// Response Types (match TypeScript interfaces)
// =============================================================================

/// Summary of a run for the run list panel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub name: String,
    pub status: RunStatus,
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    pub elapsed_minutes: f64,
    pub time_limit_minutes: Option<u32>,
    pub has_unread_messages: bool,
    pub created_at: String,
}

/// Full run details for the detail view
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetail {
    pub name: String,
    pub status: RunStatus,
    pub request: Option<String>,
    pub project_path: Option<String>,
    pub remote_url: Option<String>,
    pub branch: Option<String>,
    pub worker_scale: Option<String>,
    pub time_limit_minutes: Option<u32>,
    pub started_at: Option<String>,
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub iteration_count: u32,
    pub max_iterations: Option<u32>,
    pub human_in_the_loop: bool,
    pub waiting_reason: Option<String>,
    pub unread_count: u32,
    // Additional fields for status bar display
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    pub elapsed_minutes: f64,
    // Learnings and compaction status
    pub learnings_count: u32,
    pub learnings_processed_at: Option<String>,
}

/// Task from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub completed_at: Option<String>,
    pub parent_id: Option<String>,
    pub blocked_by: Option<Vec<String>>,
    pub tokens_used: Option<u64>,
    pub created_at: String,
}

/// Sheep avatar configuration - deterministically generated from worker name
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheepConfig {
    /// Hat type (0=none, 1=crown, 2=cowboy, 3=tophat, 4=beanie, 5=wizard, 6=chef, 7=hardhat)
    pub hat: u8,
    /// Wool fluffiness (0-3, affects wool layer count/opacity)
    pub fluffiness: u8,
    /// Body width modifier (-2 to +2)
    pub body_width: i8,
    /// Body height modifier (-2 to +2)
    pub body_height: i8,
    /// Ear position modifier (-1 to +1 for forward/back positioning)
    pub ear_position: i8,
    /// Leg length modifier (-1 to +1)
    pub leg_length: i8,
    /// Wool color hue shift (0-359 degrees, applied as CSS filter)
    pub hue_shift: u16,
    /// Glasses type (0=none, 1=round, 2=square, 3=sunglasses, 4=eyepatch)
    pub glasses: u8,
    /// Bow tie (0=none, 1=red, 2=blue, 3=gold, 4=pink)
    pub bowtie: u8,
}

impl SheepConfig {
    /// Generate deterministic config from worker name
    /// Hat 8 (detective) is reserved for eval agents
    pub fn from_name(name: &str, is_leader: bool) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        let hash = hasher.finish();

        // Use different parts of the hash for different attributes
        let bytes = hash.to_le_bytes();

        SheepConfig {
            // Leaders always get crown (1), others get random hat (0-7, where 0=none)
            // Hat 8 (detective) is reserved for eval agents
            hat: if is_leader { 1 } else { bytes[0] % 8 },
            fluffiness: bytes[1] % 4,                 // 0-3
            body_width: ((bytes[2] % 5) as i8) - 2,   // -2 to +2
            body_height: ((bytes[3] % 5) as i8) - 2,  // -2 to +2
            ear_position: ((bytes[4] % 3) as i8) - 1, // -1 to +1
            leg_length: ((bytes[5] % 3) as i8) - 1,   // -1 to +1
            hue_shift: 0,                             // disabled - looks odd
            glasses: bytes[6] % 5,                    // 0-4 (0=none most common)
            bowtie: bytes[7] % 5,                     // 0-4 (0=none most common)
        }
    }

    /// Generate deterministic config for an eval agent
    /// Always uses detective hat (8), unique appearance based on eval id
    pub fn for_eval(eval_id: u32) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        eval_id.hash(&mut hasher);
        let hash = hasher.finish();

        let bytes = hash.to_le_bytes();

        SheepConfig {
            hat: 8,                                   // Always detective hat
            fluffiness: bytes[1] % 4,                 // 0-3
            body_width: ((bytes[2] % 5) as i8) - 2,   // -2 to +2
            body_height: ((bytes[3] % 5) as i8) - 2,  // -2 to +2
            ear_position: ((bytes[4] % 3) as i8) - 1, // -1 to +1
            leg_length: ((bytes[5] % 3) as i8) - 1,   // -1 to +1
            hue_shift: 0,                             // disabled
            glasses: bytes[6] % 5,                    // 0-4 (0=none most common)
            bowtie: bytes[7] % 5,                     // 0-4 (0=none most common)
        }
    }
}

/// Worker from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worker {
    pub id: u32,
    pub name: String,
    pub pid: Option<u32>,
    pub session_id: Option<String>,
    pub status: WorkerStatus,
    pub work_dir: Option<String>,
    pub waiting_thread: Option<String>,
    pub location: WorkerLocation,
    pub last_heartbeat: Option<String>,
    pub created_at: String,
    pub needs_restart: bool,
    pub session_started_at: Option<String>,
    // Display fields
    pub is_leader: bool,
    pub context_utilization: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub turns: Option<u32>,
    pub current_task: Option<String>,
    /// Sheep avatar configuration
    pub sheep_config: SheepConfig,
}

/// Message from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: u32,
    pub thread: String,
    pub sender: String,
    pub content: String,
    pub waiting: bool,
    pub read_by: Option<Vec<String>>,
    pub timestamp: String,
}

/// Thread summary for chat panel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub name: String,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_message: Option<String>,
    pub last_timestamp: Option<String>,
}

/// Unread notification aggregated across all runs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadNotification {
    pub id: String,
    pub run_name: String,
    pub thread: String,
    pub sender: String,
    pub content: String,
    pub timestamp: String,
}

/// Response for get_all_unread_notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadNotificationsResponse {
    pub notifications: Vec<UnreadNotification>,
    pub total_runs_with_unread: u32,
}

/// History entry for activity log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: u32,
    pub timestamp: String,
    pub action: String,
    pub detail: Option<String>,
}

/// Eval from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eval {
    pub id: u32,
    pub branch: String,
    pub eval_name: Option<String>,
    pub status: EvalStatus,
    pub feedback: Option<String>,
    pub log_file: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Sheep avatar configuration (detective hat)
    pub sheep_config: SheepConfig,
}

/// Agent preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreset {
    pub name: String,
    pub command: Vec<String>,
    pub mcp_config: Option<serde_json::Value>,
}

/// Authentication method for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethodResponse {
    Env,
    ApiKey,
    OAuth,
}

impl From<config::AuthMethod> for AuthMethodResponse {
    fn from(method: config::AuthMethod) -> Self {
        match method {
            config::AuthMethod::Env => Self::Env,
            config::AuthMethod::ApiKey => Self::ApiKey,
            config::AuthMethod::OAuth => Self::OAuth,
        }
    }
}

impl From<AuthMethodResponse> for config::AuthMethod {
    fn from(method: AuthMethodResponse) -> Self {
        match method {
            AuthMethodResponse::Env => Self::Env,
            AuthMethodResponse::ApiKey => Self::ApiKey,
            AuthMethodResponse::OAuth => Self::OAuth,
        }
    }
}

/// Agent auth configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthResponse {
    pub method: AuthMethodResponse,
    pub api_key: Option<String>,
    pub env_var: Option<String>,
}

impl From<config::AgentAuth> for AgentAuthResponse {
    fn from(auth: config::AgentAuth) -> Self {
        Self {
            method: auth.method.into(),
            // Don't expose full API key, just indicate if one is set
            api_key: auth.api_key.map(|k| {
                if k.len() > 8 {
                    format!("{}...{}", &k[..4], &k[k.len() - 4..])
                } else {
                    "****".to_string()
                }
            }),
            env_var: auth.env_var,
        }
    }
}

/// Auth configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigResponse {
    pub default_method: AuthMethodResponse,
    pub claude: Option<AgentAuthResponse>,
    pub gemini: Option<AgentAuthResponse>,
    pub codex: Option<AgentAuthResponse>,
    pub goose: Option<AgentAuthResponse>,
}

impl From<config::AuthConfig> for AuthConfigResponse {
    fn from(auth: config::AuthConfig) -> Self {
        Self {
            default_method: auth.default_method.into(),
            claude: auth.claude.map(|a| a.into()),
            gemini: auth.gemini.map(|a| a.into()),
            codex: auth.codex.map(|a| a.into()),
            goose: auth.goose.map(|a| a.into()),
        }
    }
}

/// Remote configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfigResponse {
    pub host: String,
    pub ssh_key: Option<String>,
    pub ssh_port: u16,
    pub work_base: String,
    pub python_path: String,
    pub location: Option<String>,
}

impl From<config::RemoteConfig> for RemoteConfigResponse {
    fn from(remote: config::RemoteConfig) -> Self {
        Self {
            host: remote.host,
            ssh_key: remote.ssh_key,
            ssh_port: remote.ssh_port,
            work_base: remote.work_base,
            python_path: remote.python_path,
            location: remote.location,
        }
    }
}

/// Runner configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RunnerConfigResponse {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "ssh")]
    Ssh {
        host: String,
        ssh_key: Option<String>,
        ssh_port: u16,
        work_base: String,
        location: Option<String>,
    },
    #[serde(rename = "sprite")]
    Sprite {
        api_token: Option<String>,
        base_checkpoint: Option<String>,
        auto_destroy: bool,
        idle_timeout_secs: u32,
        api_url: String,
    },
}

impl From<crate::core::runner::RunnerConfig> for RunnerConfigResponse {
    fn from(cfg: crate::core::runner::RunnerConfig) -> Self {
        match cfg {
            crate::core::runner::RunnerConfig::Local => RunnerConfigResponse::Local,
            crate::core::runner::RunnerConfig::Ssh(ssh) => RunnerConfigResponse::Ssh {
                host: ssh.host,
                ssh_key: ssh.ssh_key,
                ssh_port: ssh.ssh_port,
                work_base: ssh.work_base,
                location: ssh.location,
            },
            crate::core::runner::RunnerConfig::Sprite(sprite) => RunnerConfigResponse::Sprite {
                api_token: sprite.api_token,
                base_checkpoint: sprite.base_checkpoint,
                auto_destroy: sprite.auto_destroy,
                idle_timeout_secs: sprite.idle_timeout_secs,
                api_url: sprite.api_url,
            },
        }
    }
}

impl From<RunnerConfigResponse> for crate::core::runner::RunnerConfig {
    fn from(cfg: RunnerConfigResponse) -> Self {
        match cfg {
            RunnerConfigResponse::Local => crate::core::runner::RunnerConfig::Local,
            RunnerConfigResponse::Ssh {
                host,
                ssh_key,
                ssh_port,
                work_base,
                location,
            } => crate::core::runner::RunnerConfig::Ssh(crate::core::runner::SshRunnerConfig {
                host,
                ssh_key,
                ssh_port,
                work_base,
                location,
            }),
            RunnerConfigResponse::Sprite {
                api_token,
                base_checkpoint,
                auto_destroy,
                idle_timeout_secs,
                api_url,
            } => {
                crate::core::runner::RunnerConfig::Sprite(crate::core::runner::SpriteRunnerConfig {
                    api_token,
                    base_checkpoint,
                    auto_destroy,
                    idle_timeout_secs,
                    api_url,
                })
            }
        }
    }
}

/// Application configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResponse {
    pub runs_dir: String,
    pub agent_command: Vec<String>,
    pub eval_timeout: u32,
    pub auto_learn: bool,
    pub max_iterations: Option<u32>,
    pub user_message_pause: String,
    pub human_in_the_loop: bool,
    pub compaction_enabled: bool,
    pub compaction_threshold: Option<u32>,
    pub compaction_keep_messages: u32,
    pub auto_improve: bool,
    pub context_warning_threshold: f64,
    pub coordinator_port: u16,
    pub auth: AuthConfigResponse,
    pub remotes: std::collections::HashMap<String, RemoteConfigResponse>,
    pub default_remote: Option<String>,
    pub runners: std::collections::HashMap<String, RunnerConfigResponse>,
    pub default_runner: Option<String>,
    pub worker_runners: std::collections::HashMap<String, String>,
}

/// Agent auth update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthUpdate {
    pub method: AuthMethodResponse,
    pub api_key: Option<String>,
    pub env_var: Option<String>,
}

impl From<AgentAuthUpdate> for config::AgentAuth {
    fn from(update: AgentAuthUpdate) -> Self {
        Self {
            method: update.method.into(),
            api_key: update.api_key,
            env_var: update.env_var,
        }
    }
}

/// Auth config update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigUpdate {
    pub default_method: Option<AuthMethodResponse>,
    pub claude: Option<AgentAuthUpdate>,
    pub gemini: Option<AgentAuthUpdate>,
    pub codex: Option<AgentAuthUpdate>,
    pub goose: Option<AgentAuthUpdate>,
}

/// Remote config update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfigUpdate {
    pub host: String,
    pub ssh_key: Option<String>,
    pub ssh_port: Option<u16>,
    pub work_base: Option<String>,
    pub python_path: Option<String>,
    pub location: Option<String>,
}

impl From<RemoteConfigUpdate> for config::RemoteConfig {
    fn from(update: RemoteConfigUpdate) -> Self {
        Self {
            host: update.host,
            ssh_key: update.ssh_key,
            ssh_port: update.ssh_port.unwrap_or(22),
            work_base: update
                .work_base
                .unwrap_or_else(|| "/tmp/hirsel-remote".to_string()),
            python_path: update.python_path.unwrap_or_else(|| "python3".to_string()),
            location: update.location,
        }
    }
}

/// Request to update configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdateRequest {
    pub agent_command: Option<Vec<String>>,
    pub eval_timeout: Option<u32>,
    pub auto_learn: Option<bool>,
    pub max_iterations: Option<Option<u32>>,
    pub user_message_pause: Option<String>,
    pub human_in_the_loop: Option<bool>,
    pub compaction_enabled: Option<bool>,
    pub compaction_threshold: Option<Option<u32>>,
    pub compaction_keep_messages: Option<u32>,
    pub auto_improve: Option<bool>,
    pub context_warning_threshold: Option<f64>,
    pub coordinator_port: Option<u16>,
    pub auth: Option<AuthConfigUpdate>,
    pub remotes: Option<std::collections::HashMap<String, RemoteConfigUpdate>>,
    pub default_remote: Option<Option<String>>,
    pub runners: Option<std::collections::HashMap<String, RunnerConfigResponse>>,
    pub default_runner: Option<Option<String>>,
    pub worker_runners: Option<std::collections::HashMap<String, String>>,
}

/// Result of validating a repository path/URL
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoValidation {
    /// Whether the repo is valid and accessible
    pub valid: bool,
    /// Error message if not valid
    pub error: Option<String>,
    /// Whether this is a remote URL (vs local path)
    pub is_remote: bool,
    /// Available branches in the repository
    pub branches: Vec<String>,
    /// Currently checked out branch (for local repos)
    pub current_branch: Option<String>,
    /// The normalized repo URL (with branch stripped if it was in the URL)
    pub repo_url: String,
    /// Branch extracted from URL (if any)
    pub url_branch: Option<String>,
    /// Whether the URL branch exists in the repo
    pub url_branch_valid: bool,
    /// Whether the directory needs to be created (local paths only)
    pub needs_dir_create: bool,
    /// Whether git needs to be initialized (local paths only)
    pub needs_git_init: bool,
}

// =============================================================================
// Run Commands
// =============================================================================

/// Get list of all runs
/// Optimized to use get_run_summary which fetches all data in 3 queries per run
#[tauri::command]
pub async fn get_runs() -> Result<Vec<RunSummary>, String> {
    let run_names = config::list_runs().map_err(|e| format!("list_runs error: {}", e))?;
    let mut runs = Vec::new();

    for name in run_names {
        let db_path = config::run_dir(&name).join("hirsel.db");
        if !db_path.exists() {
            continue;
        }

        match SQLiteState::new(db_path.clone()) {
            Ok(state) => {
                // Use optimized summary fetch (3 queries instead of ~9)
                let summary = match state.get_run_summary() {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // Convert core Status to GUI RunStatus
                let run_status = match summary.status {
                    crate::core::state::Status::Draft => RunStatus::Draft,
                    crate::core::state::Status::Idle => RunStatus::Idle,
                    crate::core::state::Status::Working => RunStatus::Working,
                    crate::core::state::Status::Paused => RunStatus::Paused,
                    crate::core::state::Status::Runaway => RunStatus::Runaway,
                    crate::core::state::Status::TimedOut => RunStatus::TimedOut,
                    crate::core::state::Status::Eval => RunStatus::Eval,
                    crate::core::state::Status::EvalFailed => RunStatus::EvalFailed,
                    crate::core::state::Status::Waiting => RunStatus::Waiting,
                    crate::core::state::Status::Done => RunStatus::Done,
                    crate::core::state::Status::Delivered => RunStatus::Delivered,
                    crate::core::state::Status::Merged => RunStatus::Merged,
                };

                // For completed runs, recalculate elapsed as duration (start to completion)
                // The summary returns elapsed from start to now, which is correct for active runs
                let elapsed_minutes = if is_completed_status(&run_status) {
                    let start = summary
                        .started_at
                        .as_deref()
                        .or(summary.created_at.as_deref());
                    if let (Some(start), Some(end)) = (start, summary.updated_at.as_deref()) {
                        calculate_duration_minutes(start, end)
                    } else {
                        summary.elapsed_minutes
                    }
                } else {
                    summary.elapsed_minutes
                };

                let created_at = summary
                    .created_at
                    .unwrap_or_else(|| Utc::now().to_rfc3339());

                runs.push(RunSummary {
                    name,
                    status: run_status,
                    tasks_done: summary.tasks_done,
                    tasks_total: summary.tasks_total,
                    workers_active: summary.workers_active,
                    workers_total: summary.workers_total,
                    elapsed_minutes,
                    time_limit_minutes: summary.time_limit_minutes.map(|m| m as u32),
                    has_unread_messages: summary.unread_count > 0,
                    created_at,
                });
            }
            Err(_) => continue,
        }
    }

    // Sort by created_at descending (newest first)
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(runs)
}

/// Get detailed information about a specific run
#[tauri::command]
pub async fn get_run_detail(run_name: String) -> Result<RunDetail, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let status = state.status().unwrap_or(crate::core::state::Status::Idle);
    let run_status = match status {
        crate::core::state::Status::Draft => RunStatus::Draft,
        crate::core::state::Status::Idle => RunStatus::Idle,
        crate::core::state::Status::Working => RunStatus::Working,
        crate::core::state::Status::Paused => RunStatus::Paused,
        crate::core::state::Status::Runaway => RunStatus::Runaway,
        crate::core::state::Status::TimedOut => RunStatus::TimedOut,
        crate::core::state::Status::Eval => RunStatus::Eval,
        crate::core::state::Status::EvalFailed => RunStatus::EvalFailed,
        crate::core::state::Status::Waiting => RunStatus::Waiting,
        crate::core::state::Status::Done => RunStatus::Done,
        crate::core::state::Status::Delivered => RunStatus::Delivered,
        crate::core::state::Status::Merged => RunStatus::Merged,
    };

    let request = state.get_request().ok().flatten();
    let project_path = state.get_project_path().ok().flatten();
    let worker_scale = state.get_worker_scale().ok().flatten();
    let time_limit_minutes = state
        .get_time_limit_minutes()
        .ok()
        .flatten()
        .map(|m| m as u32);
    let started_at = state.get_started_at().ok().flatten();
    let summary = state.get_summary().ok().flatten();
    let created_at = state
        .get_created_at()
        .ok()
        .flatten()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let iteration_count = state.get_iteration_count().unwrap_or(0) as u32;
    let max_iterations = state.get_max_iterations().ok().flatten().map(|m| m as u32);
    let human_in_the_loop = state.get_human_in_the_loop().unwrap_or(true);
    let waiting_reason = state.get_waiting_reason().ok().flatten();
    let unread_count = state.get_unread_count().unwrap_or(0) as u32;

    // Get tasks and workers for counts
    let tasks = state.get_tasks().unwrap_or_default();
    let workers = state.get_workers().unwrap_or_default();

    let tasks_done = tasks
        .iter()
        .filter(|t| t.status == crate::core::state::TaskStatus::Done)
        .count() as u32;
    let tasks_total = tasks.len() as u32;
    let workers_active = workers
        .iter()
        .filter(|w| w.status == crate::core::state::WorkerStatus::Working)
        .count() as u32;
    let workers_total = workers.len() as u32;

    // Calculate elapsed minutes
    let elapsed_minutes = if let Ok(Some(time_info)) = state.get_time_info() {
        time_info.elapsed_minutes
    } else if let Some(ref sa) = started_at {
        parse_elapsed_minutes(sa)
    } else {
        parse_elapsed_minutes(&created_at)
    };

    // Get remote URL if set
    let remote_url = state.get_remote_url().ok().flatten();

    // Get branch if set
    let branch = state.get_branch().ok().flatten();

    // Get learnings count (efficient COUNT query instead of fetching all)
    let learnings_count = state.get_messages_count("learnings").unwrap_or(0) as u32;
    let learnings_processed_at = state.get_learnings_processed_at().ok().flatten();

    // Trigger automatic compaction check (spawns subprocess if needed)
    // Only when run is in an active state
    if matches!(
        run_status,
        RunStatus::Working | RunStatus::Eval | RunStatus::Done
    ) {
        if let Err(e) = trigger_compaction_if_needed(&run_name) {
            tracing::debug!("Compaction check failed: {}", e);
        }
    }

    Ok(RunDetail {
        name: run_name,
        status: run_status,
        request,
        project_path,
        remote_url,
        branch,
        worker_scale,
        time_limit_minutes,
        started_at,
        summary,
        created_at: created_at.clone(),
        updated_at: created_at,
        iteration_count,
        max_iterations,
        human_in_the_loop,
        waiting_reason,
        unread_count,
        tasks_done,
        tasks_total,
        workers_active,
        workers_total,
        elapsed_minutes,
        learnings_count,
        learnings_processed_at,
    })
}

/// Pause a running run
#[tauri::command]
pub async fn pause_run(run_name: String) -> Result<(), String> {
    use crate::core::workers::pause_all_workers;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Check current status
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status == crate::core::state::Status::Paused {
        return Ok(()); // Already paused
    }
    if status != crate::core::state::Status::Working {
        return Err(format!("Cannot pause run in '{}' status", status));
    }

    // Pause all workers (sends SIGTERM)
    let paused =
        pause_all_workers(&state).map_err(|e| format!("Failed to pause workers: {}", e))?;

    // Update status
    state
        .set_status(crate::core::state::Status::Paused)
        .map_err(|e| format!("Failed to update status: {}", e))?;

    tracing::info!(
        "Paused run '{}', stopped {} workers",
        run_name,
        paused.len()
    );
    Ok(())
}

/// Resume a paused run
#[tauri::command]
pub async fn resume_run(run_name: String) -> Result<(), String> {
    use crate::cli::config::get_agent_command;
    use crate::core::workers::resume_awaiting_workers;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Check current status
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status == crate::core::state::Status::Working {
        return Ok(()); // Already running
    }
    if status != crate::core::state::Status::Paused && status != crate::core::state::Status::Runaway
    {
        return Err(format!("Cannot resume run in '{}' status", status));
    }

    // Update status first
    state
        .set_status(crate::core::state::Status::Working)
        .map_err(|e| format!("Failed to update status: {}", e))?;

    // Resume workers
    let agent_command = get_agent_command();
    let resumed = resume_awaiting_workers(&run_name, &run_dir, &agent_command)
        .map_err(|e| format!("Failed to resume workers: {}", e))?;

    tracing::info!(
        "Resumed run '{}', restarted {} workers",
        run_name,
        resumed.len()
    );
    Ok(())
}

/// Delete a run
#[tauri::command]
pub async fn delete_run(run_name: String) -> Result<(), String> {
    use crate::core::workers::kill_all_workers;
    use std::fs;

    let run_dir = config::run_dir(&run_name);

    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    // Kill any running workers first
    let db_path = run_dir.join("hirsel.db");
    if db_path.exists() {
        if let Ok(state) = SQLiteState::new(db_path) {
            if let Ok(killed) = kill_all_workers(&state) {
                if !killed.is_empty() {
                    tracing::info!("Killed {} worker(s) before deleting run", killed.len());
                }
            }
        }
    }

    // Delete the run directory
    fs::remove_dir_all(&run_dir).map_err(|e| format!("Failed to delete run directory: {}", e))?;

    tracing::info!("Deleted run '{}'", run_name);
    Ok(())
}

/// Deliver a run's changes to a branch
///
/// Creates a branch in the target repository with the run's changes.
/// For remote repos, pushes to the remote. For local repos, creates a local branch.
/// Returns the branch name on success.
#[tauri::command]
pub async fn deliver_run(run_name: String, branch_name: Option<String>) -> Result<String, String> {
    use crate::core::git;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Get project path and remote URL
    let project_path_str = state
        .get_project_path()
        .map_err(|e| format!("Failed to get project path: {}", e))?;
    let remote_url = state
        .get_remote_url()
        .map_err(|e| format!("Failed to get remote URL: {}", e))?;
    let saved_branch = state
        .get_branch()
        .map_err(|e| format!("Failed to get branch: {}", e))?;

    let project_path = project_path_str
        .as_ref()
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists())
        .ok_or_else(|| format!("Project path not found for run '{}'", run_name))?;

    // Find work directory (staging dir has the main git repo)
    let work_dir = run_dir.join("work").join("staging");
    let work_dir = if work_dir.exists() {
        work_dir
    } else {
        let fallback = run_dir.join("work");
        if fallback.exists() {
            fallback
        } else {
            return Err(format!("Work directory not found for run '{}'", run_name));
        }
    };

    if !work_dir.join(".git").exists() {
        return Err("No git repository found in work directory".into());
    }

    // Check for unmerged branches
    let unmerged = git::list_unmerged_branches(&work_dir)
        .map_err(|e| format!("Failed to check branches: {}", e))?;
    if !unmerged.is_empty() {
        let branch_list = unmerged.join(", ");
        return Err(format!(
            "Unmerged branches exist: {}. All work must be merged to 'staging' before delivering.",
            branch_list
        ));
    }

    // Determine branch name: provided > saved > default
    let branch = branch_name
        .or(saved_branch)
        .unwrap_or_else(|| format!("hirsel/{}", run_name));

    // Deliver based on whether it's a remote or local repo
    let (success, message) = if let Some(ref url) = remote_url {
        git::push_to_remote(&work_dir, url, &branch)
            .map_err(|e| format!("Failed to push to remote: {}", e))?
    } else {
        // Check if branch already exists in local project repo
        if git::branch_exists(&branch, Some(&project_path))
            .map_err(|e| format!("Failed to check branch: {}", e))?
        {
            return Err(format!(
                "Branch '{}' already exists in project repository",
                branch
            ));
        }

        git::push_staging_as_branch(&work_dir, &project_path, &branch)
            .map_err(|e| format!("Failed to create branch: {}", e))?
    };

    if !success {
        return Err(format!("Delivery failed: {}", message));
    }

    // Update run status to delivered
    state
        .set_status(crate::core::state::Status::Delivered)
        .map_err(|e| format!("Failed to update status: {}", e))?;

    tracing::info!("Delivered run '{}' to branch '{}'", run_name, branch);
    Ok(branch)
}

// =============================================================================
// Draft Commands
// =============================================================================

/// Adjectives for random run names
const ADJECTIVES: &[&str] = &[
    "curious", "swift", "bright", "calm", "bold", "eager", "gentle", "happy", "clever", "brave",
    "kind", "quick", "quiet", "wise", "warm", "keen", "noble", "merry", "fair", "steady", "agile",
    "witty", "lively", "earnest",
];

/// Nouns for random run names
const NOUNS: &[&str] = &[
    "fox", "eagle", "wolf", "owl", "bear", "hawk", "deer", "hare", "otter", "raven", "falcon",
    "lynx", "crane", "swan", "finch", "sparrow", "badger", "heron", "robin", "wren", "thrush",
    "lark", "dove", "jay",
];

/// Generate a random friendly run name like "curious-fox" or "swift-eagle"
fn generate_run_name() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Simple pseudo-random based on system time
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as usize;

    let adj_idx = seed % ADJECTIVES.len();
    let noun_idx = (seed / ADJECTIVES.len()) % NOUNS.len();

    format!("{}-{}", ADJECTIVES[adj_idx], NOUNS[noun_idx])
}

/// Request to update a draft run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftUpdateRequest {
    pub spec: Option<String>,
    pub worker_scale: Option<String>,
    pub time_limit_minutes: Option<u32>,
    pub human_in_the_loop: Option<bool>,
    pub project_path: Option<String>,
    pub name: Option<String>,
    pub branch: Option<String>,
}

/// Validate a repository path or URL
///
/// Checks if the path/URL is valid, extracts branch information from URLs,
/// and lists available branches in the repository.
#[tauri::command]
pub async fn validate_repo(path: String) -> Result<RepoValidation, String> {
    use crate::core::git::{
        get_current_branch, get_repo, is_remote_url, list_branches, list_remote_branches,
        parse_github_url,
    };

    let trimmed = path.trim();

    if trimmed.is_empty() {
        return Ok(RepoValidation {
            valid: false,
            error: Some("Path is empty".to_string()),
            is_remote: false,
            branches: vec![],
            current_branch: None,
            repo_url: String::new(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        });
    }

    let is_remote = is_remote_url(trimmed);

    if is_remote {
        // Parse the URL to extract potential branch
        let parsed = parse_github_url(trimmed);

        // Try to list remote branches
        match list_remote_branches(&parsed.repo_url) {
            Ok(branches) => {
                // Check if URL branch exists
                let url_branch_valid = if let Some(ref branch) = parsed.branch {
                    branches.iter().any(|b| b == branch)
                } else {
                    true // No branch specified is valid
                };

                // If branch was specified but doesn't exist, return error
                if parsed.branch.is_some() && !url_branch_valid {
                    return Ok(RepoValidation {
                        valid: false,
                        error: Some(format!(
                            "Branch '{}' not found in repository",
                            parsed.branch.as_ref().unwrap()
                        )),
                        is_remote: true,
                        branches,
                        current_branch: None,
                        repo_url: parsed.repo_url,
                        url_branch: parsed.branch,
                        url_branch_valid: false,
                        needs_dir_create: false,
                        needs_git_init: false,
                    });
                }

                Ok(RepoValidation {
                    valid: true,
                    error: None,
                    is_remote: true,
                    branches,
                    current_branch: None,
                    repo_url: parsed.repo_url,
                    url_branch: parsed.branch,
                    url_branch_valid,
                    needs_dir_create: false,
                    needs_git_init: false,
                })
            }
            Err(e) => Ok(RepoValidation {
                valid: false,
                error: Some(format!("Failed to access repository: {}", e)),
                is_remote: true,
                branches: vec![],
                current_branch: None,
                repo_url: parsed.repo_url,
                url_branch: parsed.branch,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: false,
            }),
        }
    } else {
        // Local path
        let path = std::path::Path::new(trimmed);

        // Check if directory needs to be created
        let needs_dir_create = !path.exists();

        // Check if git needs to be initialized (directory exists but no .git)
        // We check for .git directly, not whether it's inside a git repo
        let needs_git_init = !needs_dir_create && !path.join(".git").exists();

        // If needs setup, return with flags but valid=false
        if needs_dir_create || needs_git_init {
            return Ok(RepoValidation {
                valid: false,
                error: None, // No error - just needs setup
                is_remote: false,
                branches: vec![],
                current_branch: None,
                repo_url: trimmed.to_string(),
                url_branch: None,
                url_branch_valid: false,
                needs_dir_create,
                needs_git_init,
            });
        }

        // Check if it's a git repository (should always succeed now since we checked .git exists)
        if get_repo(Some(path)).is_err() {
            return Ok(RepoValidation {
                valid: false,
                error: Some("Failed to open git repository".to_string()),
                is_remote: false,
                branches: vec![],
                current_branch: None,
                repo_url: trimmed.to_string(),
                url_branch: None,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: false,
            });
        }

        // Get branches and current branch
        let branches = list_branches(path).unwrap_or_default();
        let current_branch = get_current_branch(path).ok();

        Ok(RepoValidation {
            valid: true,
            error: None,
            is_remote: false,
            branches,
            current_branch,
            repo_url: trimmed.to_string(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        })
    }
}

/// Initialize a project directory for use with Hirsel
///
/// Creates the directory if it doesn't exist and initializes a git repository
/// if needed. Returns updated validation info after setup.
#[tauri::command]
pub async fn init_project_repo(path: String) -> Result<RepoValidation, String> {
    use crate::core::git::{get_current_branch, get_repo, list_branches};
    use std::fs;
    use std::process::Command;

    let trimmed = path.trim();
    let path = std::path::Path::new(trimmed);

    // Create directory if it doesn't exist
    if !path.exists() {
        fs::create_dir_all(path).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Initialize git if needed
    if !path.join(".git").exists() {
        // git init -b main
        let output = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git init: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "git init failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // git add -A (add any existing files)
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git add: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "git add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // git commit -m "Initial commit" --allow-empty
        let output = Command::new("git")
            .args(["commit", "-m", "Initial commit", "--allow-empty"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git commit: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "git commit failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    // Verify git repo is now valid
    if get_repo(Some(path)).is_err() {
        return Err("Failed to initialize git repository".to_string());
    }

    // Get branches and current branch
    let branches = list_branches(path).unwrap_or_default();
    let current_branch = get_current_branch(path).ok();

    Ok(RepoValidation {
        valid: true,
        error: None,
        is_remote: false,
        branches,
        current_branch,
        repo_url: trimmed.to_string(),
        url_branch: None,
        url_branch_valid: false,
        needs_dir_create: false,
        needs_git_init: false,
    })
}

/// Create a new draft run
///
/// Creates a draft run with a random friendly name. The draft can be configured
/// before being started. No workers are spawned until start_draft is called.
#[tauri::command]
pub async fn create_draft(project_path: Option<String>) -> Result<RunDetail, String> {
    use crate::core::Files;
    use std::fs;

    // Generate a unique run name
    let mut run_name = generate_run_name();
    let mut run_dir = config::run_dir(&run_name);

    // Ensure the name is unique by appending a number if needed
    let mut counter = 1;
    while run_dir.exists() {
        run_name = format!("{}-{}", generate_run_name(), counter);
        run_dir = config::run_dir(&run_name);
        counter += 1;
        if counter > 100 {
            return Err("Failed to generate unique run name".to_string());
        }
    }

    // Create run directory
    fs::create_dir_all(&run_dir).map_err(|e| format!("Failed to create run directory: {}", e))?;

    // Initialize Files helper and create required directories
    let files = Files::new(&run_dir);
    files
        .init_dirs()
        .map_err(|e| format!("Failed to init dirs: {}", e))?;

    // Create empty spec.md
    fs::write(
        run_dir.join("spec.md"),
        "# Specification\n\nDescribe the task for the AI workers...\n",
    )
    .map_err(|e| format!("Failed to create spec file: {}", e))?;

    // Create empty tasks.md
    fs::write(
        run_dir.join("tasks.md"),
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Read spec, create exploration tasks |\n",
    ).map_err(|e| format!("Failed to create tasks file: {}", e))?;

    // Initialize database
    let db_path = run_dir.join("hirsel.db");
    let state =
        SQLiteState::new(db_path).map_err(|e| format!("Failed to create database: {}", e))?;

    // Initialize state with Draft status
    state
        .init_state(project_path.as_deref())
        .map_err(|e| format!("Failed to init state: {}", e))?;
    state
        .set_status(crate::core::state::Status::Draft)
        .map_err(|e| format!("Failed to set draft status: {}", e))?;

    // Set defaults
    state
        .set_worker_scale("1")
        .map_err(|e| format!("Failed to set worker scale: {}", e))?;
    state
        .set_human_in_the_loop(true)
        .map_err(|e| format!("Failed to set HITL: {}", e))?;

    // Add scope task
    let _ = state.add_task("scope", "Read spec, create exploration tasks", None, None);

    // Return the run detail
    let created_at = chrono::Utc::now().to_rfc3339();

    Ok(RunDetail {
        name: run_name,
        status: RunStatus::Draft,
        request: None,
        project_path,
        remote_url: None,
        branch: None,
        worker_scale: Some("1".to_string()),
        time_limit_minutes: None,
        started_at: None,
        summary: None,
        created_at: created_at.clone(),
        updated_at: created_at,
        iteration_count: 0,
        max_iterations: None,
        human_in_the_loop: true,
        waiting_reason: None,
        unread_count: 0,
        tasks_done: 0,
        tasks_total: 1,
        workers_active: 0,
        workers_total: 0,
        elapsed_minutes: 0.0,
        learnings_count: 0,
        learnings_processed_at: None,
    })
}

/// Clone an existing run to a new draft
///
/// Creates a new draft run with the same settings, spec, and eval as the source run.
/// Does not copy messages, tasks (except scope), workers, or any runtime state.
#[tauri::command]
pub async fn clone_run(source_run: String, new_name: String) -> Result<RunDetail, String> {
    use crate::core::Files;
    use std::fs;

    // Validate new name
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        return Err("New run name cannot be empty".to_string());
    }

    // Check source exists
    let source_dir = config::run_dir(&source_run);
    let source_db_path = source_dir.join("hirsel.db");
    if !source_db_path.exists() {
        return Err(format!("Source run '{}' not found", source_run));
    }

    // Check new name doesn't exist
    let new_dir = config::run_dir(&new_name);
    if new_dir.exists() {
        return Err(format!("Run '{}' already exists", new_name));
    }

    // Open source database to read settings
    let source_state = SQLiteState::new(source_db_path)
        .map_err(|e| format!("Failed to open source database: {}", e))?;

    // Read settings from source
    let project_path = source_state.get_project_path().ok().flatten();
    let worker_scale = source_state
        .get_worker_scale()
        .ok()
        .flatten()
        .unwrap_or_else(|| "1".to_string());
    let time_limit = source_state.get_time_limit_minutes().ok().flatten();
    let human_in_the_loop = source_state.get_human_in_the_loop().unwrap_or(true);
    let max_iterations = source_state.get_max_iterations().ok().flatten();

    // Read spec.md from source
    let source_spec_path = source_dir.join("spec.md");
    let spec_content = if source_spec_path.exists() {
        fs::read_to_string(&source_spec_path)
            .map_err(|e| format!("Failed to read source spec: {}", e))?
    } else {
        "# Specification\n\nDescribe the task for the AI workers...\n".to_string()
    };

    // Read eval.md from source (optional)
    let source_eval_path = source_dir.join("eval.md");
    let eval_content = if source_eval_path.exists() {
        Some(
            fs::read_to_string(&source_eval_path)
                .map_err(|e| format!("Failed to read source eval: {}", e))?,
        )
    } else {
        None
    };

    // Create new run directory
    fs::create_dir_all(&new_dir).map_err(|e| format!("Failed to create run directory: {}", e))?;

    // Initialize Files helper and create required directories
    let files = Files::new(&new_dir);
    files
        .init_dirs()
        .map_err(|e| format!("Failed to init dirs: {}", e))?;

    // Write spec.md
    fs::write(new_dir.join("spec.md"), &spec_content)
        .map_err(|e| format!("Failed to create spec file: {}", e))?;

    // Write eval.md if exists
    if let Some(eval) = &eval_content {
        fs::write(new_dir.join("eval.md"), eval)
            .map_err(|e| format!("Failed to create eval file: {}", e))?;
    }

    // Create tasks.md
    fs::write(
        new_dir.join("tasks.md"),
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Read spec, create exploration tasks |\n",
    ).map_err(|e| format!("Failed to create tasks file: {}", e))?;

    // Initialize database
    let db_path = new_dir.join("hirsel.db");
    let state =
        SQLiteState::new(db_path).map_err(|e| format!("Failed to create database: {}", e))?;

    // Initialize state with Draft status
    state
        .init_state(project_path.as_deref())
        .map_err(|e| format!("Failed to init state: {}", e))?;
    state
        .set_status(crate::core::state::Status::Draft)
        .map_err(|e| format!("Failed to set draft status: {}", e))?;

    // Copy settings
    state
        .set_worker_scale(&worker_scale)
        .map_err(|e| format!("Failed to set worker scale: {}", e))?;
    state
        .set_human_in_the_loop(human_in_the_loop)
        .map_err(|e| format!("Failed to set HITL: {}", e))?;
    if let Some(limit) = time_limit {
        state
            .set_time_limit_minutes(Some(limit))
            .map_err(|e| format!("Failed to set time limit: {}", e))?;
    }
    if let Some(max_iter) = max_iterations {
        state
            .set_max_iterations(Some(max_iter))
            .map_err(|e| format!("Failed to set max iterations: {}", e))?;
    }
    // Store spec content as request
    state
        .set_request(Some(&spec_content))
        .map_err(|e| format!("Failed to set request: {}", e))?;

    // Add scope task
    let _ = state.add_task("scope", "Read spec, create exploration tasks", None, None);

    // Return the run detail
    let created_at = chrono::Utc::now().to_rfc3339();

    Ok(RunDetail {
        name: new_name,
        status: RunStatus::Draft,
        request: Some(spec_content),
        project_path,
        remote_url: None,
        branch: None,
        worker_scale: Some(worker_scale),
        time_limit_minutes: time_limit.map(|t| t as u32),
        started_at: None,
        summary: None,
        created_at: created_at.clone(),
        updated_at: created_at,
        iteration_count: 0,
        max_iterations: max_iterations.map(|m| m as u32),
        human_in_the_loop,
        waiting_reason: None,
        unread_count: 0,
        tasks_done: 0,
        tasks_total: 1,
        workers_active: 0,
        workers_total: 0,
        elapsed_minutes: 0.0,
        learnings_count: 0,
        learnings_processed_at: None,
    })
}

/// Update a draft run's configuration
///
/// Allows updating the spec, worker scale, time limit, HITL mode, and project path
/// before the draft is started.
#[tauri::command]
pub async fn update_draft(run_name: String, updates: DraftUpdateRequest) -> Result<(), String> {
    use std::fs;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify it's a draft
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status != crate::core::state::Status::Draft {
        return Err("Can only update draft runs".to_string());
    }

    // Update spec
    if let Some(spec) = updates.spec {
        let spec_path = run_dir.join("spec.md");
        fs::write(&spec_path, &spec).map_err(|e| format!("Failed to write spec: {}", e))?;
        state
            .set_request(Some(&spec))
            .map_err(|e| format!("Failed to update request: {}", e))?;
    }

    // Update worker scale
    if let Some(scale) = updates.worker_scale {
        state
            .set_worker_scale(&scale)
            .map_err(|e| format!("Failed to update worker scale: {}", e))?;
    }

    // Update time limit
    if let Some(limit) = updates.time_limit_minutes {
        state
            .set_time_limit_minutes(Some(limit as i64))
            .map_err(|e| format!("Failed to update time limit: {}", e))?;
    }

    // Update HITL
    if let Some(hitl) = updates.human_in_the_loop {
        state
            .set_human_in_the_loop(hitl)
            .map_err(|e| format!("Failed to update HITL: {}", e))?;
    }

    // Update project path
    if let Some(path) = updates.project_path {
        state
            .set_project_path(&path)
            .map_err(|e| format!("Failed to update project path: {}", e))?;
    }

    // Update branch
    if let Some(branch) = updates.branch {
        state
            .set_branch(Some(&branch))
            .map_err(|e| format!("Failed to update branch: {}", e))?;
    }

    // Handle rename if requested
    if let Some(new_name) = updates.name {
        if new_name != run_name {
            let new_run_dir = config::run_dir(&new_name);
            if new_run_dir.exists() {
                return Err(format!("Run '{}' already exists", new_name));
            }
            fs::rename(&run_dir, &new_run_dir)
                .map_err(|e| format!("Failed to rename run: {}", e))?;
        }
    }

    Ok(())
}

/// Start a draft run
///
/// Spawns workers and transitions the draft to a running state.
/// The draft must have a project path set.
#[tauri::command]
pub async fn start_draft(run_name: String) -> Result<RunDetail, String> {
    use crate::cli::config::get_agent_command;
    use crate::cli::go::{get_available_names, WorkerScale};
    use crate::core::chats::{
        create_default_group_chat, create_default_user_chat, create_learnings_thread,
        create_worker_chat,
    };
    use crate::core::git::{
        checkout_branch_at_path, clone_remote_with_branch, create_worker_clone, create_workspace,
        get_repo_root, is_remote_url,
    };
    use crate::core::workers::{spawn_worker, WorkerError, WorkerSpawnConfig};
    use crate::core::Files;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state =
        SQLiteState::new(db_path.clone()).map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify it's a draft
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status != crate::core::state::Status::Draft {
        return Err("Can only start draft runs".to_string());
    }

    // Get project path - required for starting
    let project_path_str = state
        .get_project_path()
        .map_err(|e| format!("Failed to get project path: {}", e))?
        .ok_or_else(|| "Project path is required to start a run".to_string())?;

    // Get selected branch (optional - will use default if not set)
    let selected_branch = state
        .get_branch()
        .map_err(|e| format!("Failed to get branch: {}", e))?;

    // Handle remote URLs - clone to local directory
    let project_path = if is_remote_url(&project_path_str) {
        // Clone remote repo to run directory, checking out selected branch
        let clone_dir = run_dir.join("repo");
        let local_path =
            clone_remote_with_branch(&project_path_str, &clone_dir, selected_branch.as_deref())
                .map_err(|e| format!("Failed to clone remote repository: {}", e))?;

        // Store the remote URL for delivery
        state
            .set_remote_url(Some(&project_path_str))
            .map_err(|e| format!("Failed to store remote URL: {}", e))?;

        // Update project_path to local clone
        state
            .set_project_path(local_path.to_str().unwrap_or(&project_path_str))
            .map_err(|e| format!("Failed to update project path: {}", e))?;

        local_path
    } else {
        let project_path = std::path::PathBuf::from(&project_path_str);

        if !project_path.exists() {
            return Err(format!("Project path does not exist: {}", project_path_str));
        }

        // Verify project is a git repo
        let repo_root = get_repo_root(Some(&project_path))
            .map_err(|_| format!("Project path is not a git repository: {}", project_path_str))?;

        // Checkout selected branch in local repo if specified
        if let Some(ref branch) = selected_branch {
            checkout_branch_at_path(&repo_root, branch)
                .map_err(|e| format!("Failed to checkout branch '{}': {}", branch, e))?;
        }

        repo_root
    };

    // Parse worker scale
    let worker_scale_str = state
        .get_worker_scale()
        .map_err(|e| format!("Failed to get worker scale: {}", e))?
        .unwrap_or_else(|| "1".to_string());
    let scale = WorkerScale::parse(&worker_scale_str)
        .map_err(|e| format!("Invalid worker scale: {}", e))?;

    // Get worker names
    let initial_count = scale.initial_count();
    let worker_names = get_available_names(initial_count, &[]);

    // Determine if multi-worker mode
    let is_multi_worker = initial_count > 1 || scale.autoscale;
    let leader = if is_multi_worker {
        Some(worker_names[0].clone())
    } else {
        None
    };

    // Create workspace with staging branch
    let runs_dir = config::runs_dir();
    let workspace_dir = create_workspace(&run_name, &project_path, &runs_dir)
        .map_err(|e| format!("Failed to create workspace: {}", e))?;

    // Create worker clones/worktrees
    let mut worker_dirs: Vec<(String, std::path::PathBuf)> = Vec::new();

    for worker_name in &worker_names {
        let worker_dir = if is_multi_worker {
            create_worker_clone(
                &run_name,
                &project_path,
                worker_name,
                Some(&workspace_dir),
                &runs_dir,
            )
            .map_err(|e| format!("Failed to create worker clone: {}", e))?
        } else {
            workspace_dir.clone()
        };

        worker_dirs.push((worker_name.clone(), worker_dir.clone()));

        // Register worker in state
        state
            .add_worker(worker_name, worker_dir.to_str().unwrap_or("."), "local")
            .map_err(|e| format!("Failed to add worker: {}", e))?;
    }

    // Create chats
    let files = Files::new(&run_dir);
    let chats_dir = files.chats_dir();
    create_default_user_chat(&chats_dir)
        .map_err(|e| format!("Failed to create user chat: {}", e))?;

    if is_multi_worker {
        create_default_group_chat(&chats_dir, &worker_names, leader.as_deref())
            .map_err(|e| format!("Failed to create group chat: {}", e))?;
    }

    create_learnings_thread(&chats_dir, &worker_names)
        .map_err(|e| format!("Failed to create learnings thread: {}", e))?;

    for worker_name in &worker_names {
        create_worker_chat(&chats_dir, worker_name)
            .map_err(|e| format!("Failed to create worker chat: {}", e))?;
    }

    // Pre-claim scope for first worker
    let first_worker = &worker_names[0];
    let _ = state.claim_task("scope", first_worker);

    // Set status to working and start time tracking
    state
        .set_status(crate::core::state::Status::Working)
        .map_err(|e| format!("Failed to set status: {}", e))?;

    // Set started_at if time limit is set
    if state.get_time_limit_minutes().ok().flatten().is_some() {
        state
            .set_started_at(None)
            .map_err(|e| format!("Failed to set started_at: {}", e))?;
    }

    // Spawn worker processes
    let agent_command = get_agent_command();
    let spec_path = run_dir.join("spec.md");
    let teammates: Vec<String> = worker_names.clone();

    for (i, (worker_name, work_dir)) in worker_dirs.iter().enumerate() {
        let is_leader = i == 0 && is_multi_worker;
        let config = WorkerSpawnConfig {
            run_name: run_name.clone(),
            worker_name: worker_name.clone(),
            work_dir: work_dir.clone(),
            run_dir: run_dir.clone(),
            spec_path: spec_path.clone(),
            agent_command: agent_command.clone(),
            is_leader,
            leader_name: leader.clone(),
            teammates: if is_multi_worker {
                Some(
                    teammates
                        .iter()
                        .filter(|t| *t != worker_name)
                        .cloned()
                        .collect(),
                )
            } else {
                None
            },
            resume_session_id: None,
        };

        match spawn_worker(config, &state) {
            Ok(result) => {
                tracing::info!("Spawned worker {} (PID {})", result.worker_name, result.pid);
            }
            Err(WorkerError::RunPaused) => {
                break;
            }
            Err(e) => {
                tracing::warn!("Failed to spawn worker {}: {}", worker_name, e);
            }
        }
    }

    // Return updated run detail
    get_run_detail(run_name).await
}

// =============================================================================
// Spec/Eval File Commands (file-first editing)
// =============================================================================

/// Read the spec.md file for a run
#[tauri::command]
pub async fn read_spec_file(run_name: String) -> Result<String, String> {
    let spec_path = config::run_dir(&run_name).join("spec.md");
    if !spec_path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&spec_path).map_err(|e| format!("Failed to read spec file: {}", e))
}

/// Write the spec.md file for a run
#[tauri::command]
pub async fn write_spec_file(run_name: String, content: String) -> Result<(), String> {
    let spec_path = config::run_dir(&run_name).join("spec.md");
    std::fs::write(&spec_path, &content).map_err(|e| format!("Failed to write spec file: {}", e))
}

/// Read the eval.md file for a run
#[tauri::command]
pub async fn read_eval_file(run_name: String) -> Result<String, String> {
    let eval_path = config::run_dir(&run_name).join("eval.md");
    if !eval_path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&eval_path).map_err(|e| format!("Failed to read eval file: {}", e))
}

/// Write the eval.md file for a run
#[tauri::command]
pub async fn write_eval_file(run_name: String, content: String) -> Result<(), String> {
    let eval_path = config::run_dir(&run_name).join("eval.md");
    std::fs::write(&eval_path, &content).map_err(|e| format!("Failed to write eval file: {}", e))
}

// =============================================================================
// Asset Commands
// =============================================================================

/// Save an asset file (image, etc.) to a run's assets directory
/// Returns the filename that was saved (may differ from original if name conflict)
#[tauri::command]
pub async fn save_asset(
    run_name: String,
    filename: String,
    data: Vec<u8>,
) -> Result<String, String> {
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Find a unique filename
    let dest_filename = find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Write the file
    std::fs::write(&dest_path, &data).map_err(|e| format!("Failed to write asset: {}", e))?;

    Ok(dest_filename)
}

/// Find a unique filename in the assets directory
fn find_unique_asset_filename(dir: &std::path::Path, filename: &str) -> String {
    let dest = dir.join(filename);
    if !dest.exists() {
        return filename.to_string();
    }

    let path = std::path::Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let ext = path.extension().and_then(|s| s.to_str());

    let mut counter = 1;
    loop {
        let new_name = match ext {
            Some(e) => format!("{}-{}.{}", stem, counter, e),
            None => format!("{}-{}", stem, counter),
        };

        if !dir.join(&new_name).exists() {
            return new_name;
        }
        counter += 1;
    }
}

/// Import a file from a filesystem path into a run's assets directory
/// Used by drag-and-drop from native file manager
#[tauri::command]
pub async fn import_asset_from_path(run_name: String, file_path: String) -> Result<String, String> {
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let source_path = std::path::PathBuf::from(&file_path);
    if !source_path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    if !source_path.is_file() {
        return Err(format!("Not a file: {}", file_path));
    }

    let filename = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Find a unique filename
    let dest_filename = find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Copy the file
    std::fs::copy(&source_path, &dest_path).map_err(|e| format!("Failed to copy asset: {}", e))?;

    Ok(dest_filename)
}

/// Open the assets folder for a run in the system file browser
#[tauri::command]
pub async fn open_assets_folder(run_name: String) -> Result<(), String> {
    use crate::core::files::Files;
    use std::process::Command;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Open in system file browser (cross-platform)
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }

    Ok(())
}

/// Get the assets base URL for a run (for rendering images in markdown)
#[tauri::command]
pub async fn get_assets_path(run_name: String) -> Result<String, String> {
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    Ok(assets_dir.to_string_lossy().to_string())
}

// =============================================================================
// Task Commands
// =============================================================================

/// Get all tasks for a run
#[tauri::command]
pub async fn get_tasks(run_name: String) -> Result<Vec<Task>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let core_tasks = state
        .get_tasks()
        .map_err(|e| format!("Failed to get tasks: {}", e))?;

    let tasks = core_tasks
        .into_iter()
        .map(|t| {
            let status = match t.status {
                crate::core::state::TaskStatus::Todo => TaskStatus::Todo,
                crate::core::state::TaskStatus::Doing => TaskStatus::Doing,
                crate::core::state::TaskStatus::Done => TaskStatus::Done,
            };

            // Parse blocked_by string into Vec<String>
            let blocked_by = t.blocked_by.as_ref().map(|b| {
                b.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            });

            Task {
                id: t.id,
                description: t.name,
                status,
                claimed_by: t.claimed_by,
                claimed_at: t.claimed_at,
                completed_at: t.completed_at,
                parent_id: t.parent_id,
                blocked_by,
                tokens_used: t.tokens_used.map(|n| n as u64),
                created_at: t.created_at,
            }
        })
        .collect();

    Ok(tasks)
}

/// Add a new task
#[tauri::command]
pub async fn add_task(
    run_name: String,
    task_id: String,
    description: String,
    parent_id: Option<String>,
    blocked_by: Option<Vec<String>>,
) -> Result<Task, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Convert blocked_by from Vec<String> to Vec<&str> for state.add_task
    let blocked_by_refs: Option<Vec<&str>> = blocked_by
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let blocked_by_slice: Option<&[&str]> = blocked_by_refs.as_deref();

    // Add the task
    state
        .add_task(
            &task_id,
            &description,
            parent_id.as_deref(),
            blocked_by_slice,
        )
        .map_err(|e| format!("Failed to add task: {}", e))?;

    // Return the created task
    let task = state
        .get_task(&task_id)
        .map_err(|e| format!("Failed to get task: {}", e))?
        .ok_or_else(|| "Task not found after creation".to_string())?;

    // Convert blocked_by from comma-separated string to Vec
    let blocked_by_vec = task
        .blocked_by
        .as_ref()
        .map(|s| {
            s.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|v: &Vec<String>| !v.is_empty());

    Ok(Task {
        id: task.id,
        description: task.name,
        status: match task.status {
            crate::core::state::TaskStatus::Todo => TaskStatus::Todo,
            crate::core::state::TaskStatus::Doing => TaskStatus::Doing,
            crate::core::state::TaskStatus::Done => TaskStatus::Done,
        },
        claimed_by: task.claimed_by,
        claimed_at: task.claimed_at,
        completed_at: task.completed_at,
        parent_id: task.parent_id,
        blocked_by: blocked_by_vec,
        tokens_used: task.tokens_used.map(|t| t as u64),
        created_at: task.created_at,
    })
}

/// Delete a task
#[tauri::command]
pub async fn delete_task(run_name: String, task_id: String) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    state
        .delete_task(&task_id)
        .map_err(|e| format!("Failed to delete task: {}", e))?;

    Ok(())
}

/// Mark a task as complete (from UI - uses "user" as worker name)
#[tauri::command]
pub async fn complete_task(run_name: String, task_id: String) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Use "user" as the worker name for UI-initiated completions
    state
        .complete_task(&task_id, "user")
        .map_err(|e| format!("Failed to complete task: {}", e))?;

    Ok(())
}

/// Unclaim a task (release it back to the pool)
#[tauri::command]
pub async fn unclaim_task(run_name: String, task_id: String) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Get the task to find who claimed it
    let task = state
        .get_task(&task_id)
        .map_err(|e| format!("Failed to get task: {}", e))?
        .ok_or_else(|| format!("Task '{}' not found", task_id))?;

    let worker = task.claimed_by.unwrap_or_else(|| "user".to_string());

    state
        .unclaim_task(&task_id, &worker)
        .map_err(|e| format!("Failed to unclaim task: {}", e))?;

    Ok(())
}

/// Reopen a completed task
#[tauri::command]
pub async fn reopen_task(run_name: String, task_id: String) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    state
        .reopen_task(&task_id)
        .map_err(|e| format!("Failed to reopen task: {}", e))?;

    Ok(())
}

// =============================================================================
// Worker Commands
// =============================================================================

/// Get all workers for a run
#[tauri::command]
pub async fn get_workers(run_name: String) -> Result<Vec<Worker>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let core_workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;

    // Get tasks to find current task for each worker
    let tasks = state.get_tasks().unwrap_or_default();

    let workers = core_workers
        .into_iter()
        .map(|w| {
            let status = match w.status {
                crate::core::state::WorkerStatus::Idle => WorkerStatus::Idle,
                crate::core::state::WorkerStatus::Working => WorkerStatus::Working,
                crate::core::state::WorkerStatus::Waiting => WorkerStatus::Waiting,
                crate::core::state::WorkerStatus::Awaiting => WorkerStatus::Awaiting,
                crate::core::state::WorkerStatus::Paused => WorkerStatus::Paused,
                crate::core::state::WorkerStatus::Error => WorkerStatus::Error,
            };

            let location = match w.location.as_str() {
                "remote" => WorkerLocation::Remote,
                _ => WorkerLocation::Local,
            };

            // Find current task for this worker
            let current_task = tasks
                .iter()
                .find(|t| {
                    t.claimed_by.as_deref() == Some(&w.name)
                        && t.status == crate::core::state::TaskStatus::Doing
                })
                .map(|t| t.name.clone());

            // Check if this is the leader (first worker or worker id 1)
            let is_leader = w.id == 1;

            // Get session metrics for this worker
            let session_metrics =
                metrics::get_session_metrics(w.session_id.as_deref(), w.work_dir.as_deref());

            Worker {
                id: w.id as u32,
                name: w.name.clone(),
                pid: w.pid.map(|p| p as u32),
                session_id: w.session_id,
                status,
                work_dir: w.work_dir,
                waiting_thread: w.waiting_thread,
                location,
                last_heartbeat: w.last_heartbeat,
                created_at: w.created_at,
                needs_restart: w.needs_restart,
                session_started_at: w.session_started_at,
                is_leader,
                context_utilization: session_metrics.context_utilization,
                input_tokens: Some(session_metrics.input_tokens),
                output_tokens: Some(session_metrics.output_tokens),
                turns: Some(session_metrics.turns),
                current_task,
                sheep_config: SheepConfig::from_name(&w.name, is_leader),
            }
        })
        .collect();

    Ok(workers)
}

/// Attach a new worker to a run
///
/// Creates a new worker with the given name, sets up its working directory,
/// and spawns the worker process.
#[tauri::command]
pub async fn attach_worker(run_name: String, worker_name: String) -> Result<Worker, String> {
    use crate::cli::config::get_agent_command;
    use crate::core::git::create_worker_clone;
    use crate::core::workers::{spawn_worker, WorkerSpawnConfig};
    use crate::core::Files;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Check if worker already exists
    if state.get_worker(&worker_name).ok().flatten().is_some() {
        return Err(format!("Worker '{}' already exists", worker_name));
    }

    // Get project path
    let project_path_str = state
        .get_project_path()
        .map_err(|e| format!("Failed to get project path: {}", e))?
        .ok_or_else(|| "No project path configured".to_string())?;
    let project_path = std::path::PathBuf::from(&project_path_str);

    // Get existing workers to determine if multi-worker
    let workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;
    let is_multi_worker = !workers.is_empty();

    // Create worker clone/worktree
    let staging_dir = run_dir.join("work").join("staging");
    let runs_dir = config::runs_dir();
    let worker_dir = create_worker_clone(
        &run_name,
        &project_path,
        &worker_name,
        Some(&staging_dir),
        &runs_dir,
    )
    .map_err(|e| format!("Failed to create worker clone: {}", e))?;

    // Add worker to state
    state
        .add_worker(&worker_name, worker_dir.to_str().unwrap_or("."), "local")
        .map_err(|e| format!("Failed to add worker: {}", e))?;

    // Create worker chat file
    let files = Files::new(&run_dir);
    let chat_file = files.chats_dir().join(format!("{}.md", worker_name));
    let _ = std::fs::write(&chat_file, format!("# {} Chat\n\n", worker_name));

    // Get leader info
    let leader_name = workers.first().map(|w| w.name.clone());
    let teammates: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();

    // Spawn the worker
    let agent_command = get_agent_command();
    let config = WorkerSpawnConfig {
        run_name: run_name.clone(),
        worker_name: worker_name.clone(),
        work_dir: worker_dir.clone(),
        run_dir: run_dir.clone(),
        spec_path: files.spec(),
        agent_command,
        is_leader: false,
        leader_name,
        teammates: if is_multi_worker {
            Some(teammates)
        } else {
            None
        },
        resume_session_id: None,
    };

    match spawn_worker(config, &state) {
        Ok(result) => {
            tracing::info!("Attached worker {} (PID {})", worker_name, result.pid);
        }
        Err(e) => {
            return Err(format!("Failed to spawn worker: {}", e));
        }
    }

    // Return the created worker
    let worker = state
        .get_worker(&worker_name)
        .map_err(|e| format!("Failed to get worker: {}", e))?
        .ok_or_else(|| "Worker not found after creation".to_string())?;

    Ok(Worker {
        id: worker.id as u32,
        name: worker.name.clone(),
        pid: worker.pid.map(|p| p as u32),
        session_id: worker.session_id,
        status: match worker.status {
            crate::core::state::WorkerStatus::Idle => WorkerStatus::Idle,
            crate::core::state::WorkerStatus::Working => WorkerStatus::Working,
            crate::core::state::WorkerStatus::Waiting => WorkerStatus::Waiting,
            crate::core::state::WorkerStatus::Awaiting => WorkerStatus::Awaiting,
            crate::core::state::WorkerStatus::Paused => WorkerStatus::Paused,
            crate::core::state::WorkerStatus::Error => WorkerStatus::Error,
        },
        work_dir: worker.work_dir,
        waiting_thread: worker.waiting_thread,
        location: WorkerLocation::Local,
        last_heartbeat: worker.last_heartbeat,
        created_at: worker.created_at,
        needs_restart: worker.needs_restart,
        session_started_at: worker.session_started_at,
        is_leader: false,
        context_utilization: None,
        input_tokens: None,
        output_tokens: None,
        turns: None,
        current_task: None,
        sheep_config: SheepConfig::from_name(&worker.name, false),
    })
}

/// Open an external terminal attached to a worker's tmux session
///
/// This opens a new terminal window running `tmux attach-session` for the worker.
#[tauri::command]
pub async fn open_worker_terminal(run_name: String, worker_name: String) -> Result<(), String> {
    use std::process::Command;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify worker exists
    let workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;

    if !workers.iter().any(|w| w.name == worker_name) {
        return Err(format!("Worker '{}' not found", worker_name));
    }

    // Check if tmux session exists
    let session_name = format!("hirsel-{}-{}", run_name, worker_name);
    let session_exists = Command::new("tmux")
        .args(["has-session", "-t", &session_name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !session_exists {
        // Check for log file as fallback
        let log_file = run_dir.join("logs").join(format!("{}.log", worker_name));
        if log_file.exists() {
            return Err(format!(
                "No tmux session '{}' found. Worker may not be running.\n\nYou can view the log with:\n  tail -f {}",
                session_name,
                log_file.display()
            ));
        }
        return Err(format!(
            "No tmux session '{}' found. Worker '{}' may not be running.",
            session_name, worker_name
        ));
    }

    // Try to open a terminal with tmux attach
    // Try common terminal emulators in order of preference
    let attach_cmd = format!("tmux attach-session -t {}", session_name);

    let terminals = [
        ("alacritty", vec!["-e", "sh", "-c", &attach_cmd]),
        ("kitty", vec!["sh", "-c", &attach_cmd]),
        ("wezterm", vec!["start", "--", "sh", "-c", &attach_cmd]),
        ("gnome-terminal", vec!["--", "sh", "-c", &attach_cmd]),
        ("konsole", vec!["-e", "sh", "-c", &attach_cmd]),
        ("xterm", vec!["-e", "sh", "-c", &attach_cmd]),
        ("x-terminal-emulator", vec!["-e", "sh", "-c", &attach_cmd]),
    ];

    for (term, args) in &terminals {
        if Command::new("which")
            .arg(term)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            match Command::new(term).args(args).spawn() {
                Ok(_) => return Ok(()),
                Err(_) => continue,
            }
        }
    }

    Err(format!(
        "Could not find a terminal emulator. Run manually:\n  tmux attach-session -t {}",
        session_name
    ))
}

/// Detach/stop a worker
///
/// Stops the worker process and marks it as paused.
#[tauri::command]
pub async fn detach_worker(run_name: String, worker_id: u32) -> Result<(), String> {
    use crate::core::state::WorkerUpdate;
    use crate::core::workers::is_pid_alive;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Find the worker by ID
    let workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;

    let worker = workers
        .iter()
        .find(|w| w.id as u32 == worker_id)
        .ok_or_else(|| format!("Worker with ID {} not found", worker_id))?;

    // Kill the process if it's running
    if let Some(pid) = worker.pid {
        if is_pid_alive(pid as u32) {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            tracing::info!("Stopped worker {} (PID {})", worker.name, pid);
        }
    }

    // Mark as paused
    state
        .update_worker(
            &worker.name,
            WorkerUpdate {
                pid: None,
                status: Some(crate::core::state::WorkerStatus::Paused),
                ..Default::default()
            },
        )
        .map_err(|e| format!("Failed to update worker: {}", e))?;

    tracing::info!("Detached worker {} from run {}", worker.name, run_name);
    Ok(())
}

/// Restart a worker
///
/// Stops the current worker process and spawns a new one.
#[tauri::command]
pub async fn restart_worker(run_name: String, worker_id: u32) -> Result<(), String> {
    use crate::cli::config::get_agent_command;
    use crate::core::state::WorkerUpdate;
    use crate::core::workers::{is_pid_alive, spawn_worker, WorkerSpawnConfig};
    use crate::core::Files;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Find the worker by ID
    let workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;

    let worker = workers
        .iter()
        .find(|w| w.id as u32 == worker_id)
        .ok_or_else(|| format!("Worker with ID {} not found", worker_id))?
        .clone();

    // Kill the process if it's running
    if let Some(pid) = worker.pid {
        if is_pid_alive(pid as u32) {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            tracing::info!("Stopped worker {} (PID {}) for restart", worker.name, pid);
            // Give the process time to clean up
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // Clear PID before restarting
    state
        .update_worker(
            &worker.name,
            WorkerUpdate {
                pid: None,
                ..Default::default()
            },
        )
        .map_err(|e| format!("Failed to update worker: {}", e))?;

    // Get work directory
    let work_dir = worker
        .work_dir
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| run_dir.join("work").join(&worker.name));

    // Determine if multi-worker mode
    let is_multi_worker = workers.len() > 1;
    let leader_name = workers.first().map(|w| w.name.clone());
    let teammates: Vec<String> = workers
        .iter()
        .filter(|w| w.name != worker.name)
        .map(|w| w.name.clone())
        .collect();

    // Spawn the worker
    let files = Files::new(&run_dir);
    let agent_command = get_agent_command();
    let config = WorkerSpawnConfig {
        run_name: run_name.clone(),
        worker_name: worker.name.clone(),
        work_dir,
        run_dir: run_dir.clone(),
        spec_path: files.spec(),
        agent_command,
        is_leader: worker.id == 1, // First worker is typically the leader
        leader_name,
        teammates: if is_multi_worker {
            Some(teammates)
        } else {
            None
        },
        resume_session_id: worker.session_id.clone(),
    };

    match spawn_worker(config, &state) {
        Ok(result) => {
            tracing::info!("Restarted worker {} (PID {})", worker.name, result.pid);
            Ok(())
        }
        Err(e) => Err(format!("Failed to restart worker: {}", e)),
    }
}

// =============================================================================
// Message Commands
// =============================================================================

/// Get messages for a thread
#[tauri::command]
pub async fn get_messages(
    run_name: String,
    thread_name: String,
    limit: Option<u32>,
) -> Result<Vec<Message>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(100) as i64;
    let core_messages = state
        .get_messages(&thread_name, limit)
        .map_err(|e| format!("Failed to get messages: {}", e))?;

    let messages = core_messages
        .into_iter()
        .map(|m| Message {
            id: m.id as u32,
            thread: m.thread,
            sender: m.sender,
            content: m.content,
            waiting: m.waiting,
            read_by: None,
            timestamp: m.timestamp,
        })
        .collect();

    Ok(messages)
}

/// Get all threads for a run
#[tauri::command]
pub async fn get_threads(run_name: String) -> Result<Vec<ThreadSummary>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let thread_names = state
        .get_threads()
        .map_err(|e| format!("Failed to get threads: {}", e))?;

    let mut threads = Vec::new();
    for name in thread_names {
        let message_count = state.get_thread_message_count(&name).unwrap_or(0) as u32;
        let messages = state.get_messages(&name, 1).unwrap_or_default();
        let last_message = messages.first().map(|m| m.content.clone());
        let last_timestamp = messages.first().map(|m| m.timestamp.clone());

        threads.push(ThreadSummary {
            name,
            message_count,
            unread_count: 0,
            last_message,
            last_timestamp,
        });
    }

    Ok(threads)
}

/// Get all unread notifications across all runs
/// This is a single query replacement for the N+1 query pattern
#[tauri::command]
pub async fn get_all_unread_notifications() -> Result<UnreadNotificationsResponse, String> {
    // Get all run directories
    let run_names = config::list_runs().unwrap_or_default();

    let mut all_notifications: Vec<UnreadNotification> = Vec::new();
    let mut runs_with_unread = 0;

    for run_name in run_names {
        let db_path = config::run_dir(&run_name).join("hirsel.db");
        if !db_path.exists() {
            continue;
        }

        let state = match SQLiteState::new(db_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Get all unread messages for the user in this run
        let unread_messages = match state.get_all_unread_messages("user") {
            Ok(m) => m,
            Err(_) => continue,
        };

        if !unread_messages.is_empty() {
            runs_with_unread += 1;

            for msg in unread_messages {
                // Skip user messages (they're not notifications)
                if msg.sender == "user" || msg.sender == "admin" || msg.sender == "system" {
                    continue;
                }

                all_notifications.push(UnreadNotification {
                    id: format!("{}-{}-{}", run_name, msg.thread, msg.id),
                    run_name: run_name.clone(),
                    thread: msg.thread,
                    sender: msg.sender,
                    content: msg.content,
                    timestamp: msg.timestamp,
                });
            }
        }
    }

    // Sort by timestamp, newest first
    all_notifications.sort_by(|a, b| {
        let a_time = parse_timestamp(&a.timestamp);
        let b_time = parse_timestamp(&b.timestamp);
        match (b_time, a_time) {
            (Some(bt), Some(at)) => bt.cmp(&at),
            _ => std::cmp::Ordering::Equal,
        }
    });

    // Limit to 100 most recent (frontend also caps at 100)
    all_notifications.truncate(100);

    Ok(UnreadNotificationsResponse {
        notifications: all_notifications,
        total_runs_with_unread: runs_with_unread,
    })
}

/// Send a message to a thread
#[tauri::command]
pub async fn send_message(
    run_name: String,
    thread_name: String,
    content: String,
) -> Result<Message, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Add the message (user messages are not waiting)
    let message_id = state
        .add_message(&thread_name, "user", &content, false)
        .map_err(|e| format!("Failed to send message: {}", e))?;

    // Return the created message
    Ok(Message {
        id: message_id as u32,
        thread: thread_name,
        sender: "user".to_string(),
        content,
        waiting: false,
        read_by: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Mark messages as read
#[tauri::command]
pub async fn mark_messages_read(
    run_name: String,
    thread_name: String,
    reader: String,
) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Mark all messages in thread as read by this reader
    state
        .mark_messages_read(&thread_name, &reader, None)
        .map_err(|e| format!("Failed to mark messages read: {}", e))?;

    Ok(())
}

// =============================================================================
// History Commands
// =============================================================================

/// Get history entries for a run
#[tauri::command]
pub async fn get_history(
    run_name: String,
    limit: Option<u32>,
) -> Result<Vec<HistoryEntry>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(100) as i64;
    let core_history = state
        .get_history(limit)
        .map_err(|e| format!("Failed to get history: {}", e))?;

    let history = core_history
        .into_iter()
        .map(|h| HistoryEntry {
            id: h.id as u32,
            timestamp: h.timestamp,
            action: h.action,
            detail: h.detail,
        })
        .collect();

    Ok(history)
}

// =============================================================================
// Eval Commands
// =============================================================================

/// Get the eval spec (eval.md) content for a run
#[tauri::command]
pub async fn get_eval_spec(run_name: String) -> Result<Option<String>, String> {
    let run_dir = config::run_dir(&run_name);
    let eval_spec_path = run_dir.join("eval.md");

    if !eval_spec_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&eval_spec_path)
        .map_err(|e| format!("Failed to read eval spec: {}", e))?;

    Ok(Some(content))
}

/// Get evals for a run
#[tauri::command]
pub async fn get_evals(run_name: String) -> Result<Vec<Eval>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let core_evals = state
        .get_evals(100)
        .map_err(|e| format!("Failed to get evals: {}", e))?;

    let evals = core_evals
        .into_iter()
        .map(|e| {
            let status = match e.status {
                crate::core::state::EvalStatus::Running => EvalStatus::Running,
                crate::core::state::EvalStatus::Passed => EvalStatus::Passed,
                crate::core::state::EvalStatus::Failed => EvalStatus::Failed,
            };

            Eval {
                id: e.id as u32,
                branch: e.branch.clone(),
                eval_name: e.eval_name,
                status,
                feedback: e.feedback,
                log_file: e.log_file,
                started_at: e.started_at,
                finished_at: e.finished_at,
                sheep_config: SheepConfig::for_eval(e.id as u32),
            }
        })
        .collect();

    Ok(evals)
}

// =============================================================================
// Worker Log Commands
// =============================================================================

/// Response for worker log content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerLogResponse {
    pub content: String,
    pub byte_offset: u64,
    pub file_size: u64,
    pub exists: bool,
}

/// Parsed log line with tool activity info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLogLine {
    pub text: String,
    pub is_tool_start: bool,
    pub is_tool_end: bool,
    pub tool_name: Option<String>,
}

/// Get worker log file content
///
/// Returns the log content for a specific worker. Supports optional line limit
/// and byte offset for efficient tailing/streaming.
#[tauri::command]
pub async fn get_worker_log(
    run_name: String,
    worker_name: String,
    lines: Option<u32>,
    from_offset: Option<u64>,
) -> Result<WorkerLogResponse, String> {
    use std::io::{Read, Seek, SeekFrom};

    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.worker_log(&worker_name);

    if !log_path.exists() {
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&log_path)
        .map_err(|e| format!("Failed to read log file metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file =
        std::fs::File::open(&log_path).map_err(|e| format!("Failed to open log file: {}", e))?;

    // If offset is provided, seek to that position
    let _start_offset = if let Some(offset) = from_offset {
        if offset < file_size {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("Failed to seek in log file: {}", e))?;
            offset
        } else {
            // Already at or past end
            return Ok(WorkerLogResponse {
                content: String::new(),
                byte_offset: file_size,
                file_size,
                exists: true,
            });
        }
    } else {
        0
    };

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read log file: {}", e))?;

    // If lines limit is specified and no offset was given, return only the last N lines
    if let (Some(limit), None) = (lines, from_offset) {
        let limit = limit as usize;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > limit {
            content = all_lines[all_lines.len() - limit..].join("\n");
        }
    }

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get worker log file path
///
/// Returns the absolute path to the worker's log file for use with
/// file system watchers or external tools.
#[tauri::command]
pub async fn get_worker_log_path(run_name: String, worker_name: String) -> Result<String, String> {
    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.worker_log(&worker_name);
    Ok(log_path.to_string_lossy().to_string())
}

/// Parse log content and extract tool activity markers
///
/// Parses `[tool:name]` and `[/tool]` markers from Claude Code output.
#[tauri::command]
pub async fn parse_worker_log(content: String) -> Result<Vec<ParsedLogLine>, String> {
    let tool_start_re =
        regex::Regex::new(r"\[tool:([^\]]+)\]").map_err(|e| format!("Invalid regex: {}", e))?;
    let tool_end_re =
        regex::Regex::new(r"\[/tool\]").map_err(|e| format!("Invalid regex: {}", e))?;

    let parsed: Vec<ParsedLogLine> = content
        .lines()
        .map(|line| {
            let is_tool_start = tool_start_re.is_match(line);
            let is_tool_end = tool_end_re.is_match(line);
            let tool_name = if is_tool_start {
                tool_start_re.captures(line).map(|c| c[1].to_string())
            } else {
                None
            };

            ParsedLogLine {
                text: line.to_string(),
                is_tool_start,
                is_tool_end,
                tool_name,
            }
        })
        .collect();

    Ok(parsed)
}

/// Get eval log file content
#[tauri::command]
pub async fn get_eval_log(
    run_name: String,
    lines: Option<u32>,
    from_offset: Option<u64>,
) -> Result<WorkerLogResponse, String> {
    use std::io::{Read, Seek, SeekFrom};

    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.eval_log();

    if !log_path.exists() {
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&log_path)
        .map_err(|e| format!("Failed to read eval log metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file =
        std::fs::File::open(&log_path).map_err(|e| format!("Failed to open eval log: {}", e))?;

    let _start_offset = if let Some(offset) = from_offset {
        if offset < file_size {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("Failed to seek in eval log: {}", e))?;
            offset
        } else {
            return Ok(WorkerLogResponse {
                content: String::new(),
                byte_offset: file_size,
                file_size,
                exists: true,
            });
        }
    } else {
        0
    };

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read eval log: {}", e))?;

    if let (Some(limit), None) = (lines, from_offset) {
        let limit = limit as usize;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > limit {
            content = all_lines[all_lines.len() - limit..].join("\n");
        }
    }

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get a specific eval's log file content by path
#[tauri::command]
pub async fn get_eval_log_by_path(
    run_name: String,
    log_file: String,
) -> Result<WorkerLogResponse, String> {
    use std::io::Read;
    use std::path::PathBuf;

    let run_dir = config::run_dir(&run_name);

    // Security: ensure the log file is within the run directory
    let log_path = PathBuf::from(&log_file);
    let canonical_run_dir = run_dir
        .canonicalize()
        .map_err(|e| format!("Failed to resolve run directory: {}", e))?;

    // If log_file is a relative path, resolve it relative to run_dir
    let resolved_log_path = if log_path.is_relative() {
        run_dir.join(&log_path)
    } else {
        log_path.clone()
    };

    // Verify the resolved path is within the run directory
    let canonical_log_path = resolved_log_path
        .canonicalize()
        .map_err(|e| format!("Log file not found: {}", e))?;

    if !canonical_log_path.starts_with(&canonical_run_dir) {
        return Err("Log file must be within the run directory".to_string());
    }

    if !canonical_log_path.exists() {
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&canonical_log_path)
        .map_err(|e| format!("Failed to read log metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file = std::fs::File::open(&canonical_log_path)
        .map_err(|e| format!("Failed to open log file: {}", e))?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read log file: {}", e))?;

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

// =============================================================================
// Worker Events Commands (ACP-based streaming)
// =============================================================================

/// Worker event for real-time streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventResponse {
    pub id: i64,
    pub worker_name: String,
    pub event_type: String,
    pub timestamp: String,
    /// Text content (for text/thought events)
    pub content: Option<String>,
    /// Tool call ID (for tool events)
    pub tool_call_id: Option<String>,
    /// Tool title/name
    pub tool_title: Option<String>,
    /// Tool kind (read, edit, execute, search, etc.)
    pub tool_kind: Option<String>,
    /// Tool execution status (pending, in_progress, completed, failed)
    pub tool_status: Option<String>,
    /// Tool input (JSON string)
    pub tool_input: Option<String>,
    /// Tool output (JSON string)
    pub tool_output: Option<String>,
}

/// Response for worker events query
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventsResponse {
    pub events: Vec<WorkerEventResponse>,
    pub last_id: Option<i64>,
    /// Worker status for determining if still streaming
    pub worker_status: Option<String>,
}

/// Get worker events for real-time streaming
///
/// Returns events since `after_id` for efficient polling.
/// On first call, pass `after_id: null` to get recent events.
#[tauri::command]
pub async fn get_worker_events(
    run_name: String,
    worker_name: String,
    after_id: Option<i64>,
    limit: Option<i64>,
) -> Result<WorkerEventsResponse, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Ok(WorkerEventsResponse {
            events: Vec::new(),
            last_id: None,
            worker_status: None,
        });
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(1000);
    let events = state
        .get_worker_events(&worker_name, after_id, limit)
        .map_err(|e| format!("Failed to get worker events: {}", e))?;

    let last_id = events.last().map(|e| e.id);

    // Get worker status to determine if still streaming
    let worker_status = state
        .get_worker(&worker_name)
        .ok()
        .flatten()
        .map(|w| w.status.as_str().to_string());

    let events: Vec<WorkerEventResponse> = events
        .into_iter()
        .map(|e| WorkerEventResponse {
            id: e.id,
            worker_name: e.worker_name,
            event_type: e.event_type.as_str().to_string(),
            timestamp: e.timestamp,
            content: e.content,
            tool_call_id: e.tool_call_id,
            tool_title: e.tool_title,
            tool_kind: e.tool_kind,
            tool_status: e.tool_status.map(|s| s.as_str().to_string()),
            tool_input: e.tool_input,
            tool_output: e.tool_output,
        })
        .collect();

    Ok(WorkerEventsResponse {
        events,
        last_id,
        worker_status,
    })
}

/// Clear worker events (for cleanup when attaching/detaching)
#[tauri::command]
pub async fn clear_worker_events(run_name: String, worker_name: String) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Ok(());
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    state
        .clear_worker_events(&worker_name)
        .map_err(|e| format!("Failed to clear worker events: {}", e))?;

    Ok(())
}

// =============================================================================
// Worker Event Streaming
// =============================================================================

use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

/// Manages active worker event streams
pub struct WorkerEventStreamManager {
    /// Active streams: (run_name, worker_name) -> cancel sender
    streams: Mutex<HashMap<(String, String), oneshot::Sender<()>>>,
}

impl WorkerEventStreamManager {
    pub fn new() -> Self {
        Self {
            streams: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a stream is active for this worker
    pub fn is_active(&self, run_name: &str, worker_name: &str) -> bool {
        let streams = self.streams.lock().unwrap();
        streams.contains_key(&(run_name.to_string(), worker_name.to_string()))
    }

    /// Register a new stream
    pub fn register(&self, run_name: &str, worker_name: &str, cancel_tx: oneshot::Sender<()>) {
        let mut streams = self.streams.lock().unwrap();
        streams.insert((run_name.to_string(), worker_name.to_string()), cancel_tx);
    }

    /// Stop and remove a stream
    pub fn stop(&self, run_name: &str, worker_name: &str) -> bool {
        let mut streams = self.streams.lock().unwrap();
        if let Some(cancel_tx) = streams.remove(&(run_name.to_string(), worker_name.to_string())) {
            // Send cancel signal (ignore error if receiver dropped)
            let _ = cancel_tx.send(());
            true
        } else {
            false
        }
    }

    /// Remove a stream without sending cancel (for cleanup after stream ends)
    pub fn remove(&self, run_name: &str, worker_name: &str) {
        let mut streams = self.streams.lock().unwrap();
        streams.remove(&(run_name.to_string(), worker_name.to_string()));
    }
}

impl Default for WorkerEventStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Worker event emitted to frontend
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventEmit {
    pub run_name: String,
    pub worker_name: String,
    #[serde(flatten)]
    pub event: WorkerEventResponse,
}

/// Stream status event emitted to frontend
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WorkerStreamEvent {
    /// Initial batch of historical events
    #[serde(rename = "history")]
    History {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
        events: Vec<WorkerEventResponse>,
        #[serde(rename = "workerStatus")]
        worker_status: Option<String>,
    },
    /// New event during live streaming
    #[serde(rename = "event")]
    Event {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
        event: WorkerEventResponse,
    },
    /// Worker status update
    #[serde(rename = "status")]
    Status {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
        #[serde(rename = "workerStatus")]
        worker_status: Option<String>,
    },
    /// Stream ended
    #[serde(rename = "ended")]
    Ended {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
    },
}

/// Start streaming worker events to the frontend
///
/// This fetches historical events first, then polls for new ones.
/// Events are emitted as `worker-event` Tauri events.
#[tauri::command]
pub async fn start_worker_event_stream(
    app: tauri::AppHandle,
    stream_manager: tauri::State<'_, std::sync::Arc<WorkerEventStreamManager>>,
    run_name: String,
    worker_name: String,
) -> Result<(), String> {
    use tauri::Emitter;

    info!(
        "[WorkerStream] start_worker_event_stream called for {}/{}",
        run_name, worker_name
    );

    // Stop any existing stream for this worker
    stream_manager.stop(&run_name, &worker_name);

    let db_path = config::run_dir(&run_name).join("hirsel.db");
    info!(
        "[WorkerStream] DB path: {:?}, exists: {}",
        db_path,
        db_path.exists()
    );
    if !db_path.exists() {
        return Err(format!("Run database not found: {}", run_name));
    }

    // Create cancel channel
    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    stream_manager.register(&run_name, &worker_name, cancel_tx);

    let run_name_clone = run_name.clone();
    let worker_name_clone = worker_name.clone();
    let stream_manager_clone = stream_manager.inner().clone();
    let app_clone = app.clone();

    // Spawn background task to stream events
    info!("[WorkerStream] Spawning background task");
    tokio::spawn(async move {
        let app = app_clone;
        info!(
            "[WorkerStream] Background task started for {}/{}",
            run_name_clone, worker_name_clone
        );
        let mut last_id: Option<i64> = None;
        let poll_interval = tokio::time::Duration::from_millis(200);
        let mut first_poll = true;

        loop {
            // Check for cancellation
            if cancel_rx.try_recv().is_ok() {
                info!(
                    "[WorkerStream] Cancelled for {}/{}",
                    run_name_clone, worker_name_clone
                );
                break;
            }

            // Poll for events
            let state = match SQLiteState::new(db_path.clone()) {
                Ok(s) => s,
                Err(e) => {
                    info!("[WorkerStream] Failed to open database: {}", e);
                    break;
                }
            };

            let events = match state.get_worker_events(&worker_name_clone, last_id, 1000) {
                Ok(e) => e,
                Err(e) => {
                    info!("[WorkerStream] Failed to get events: {}", e);
                    break;
                }
            };

            if first_poll {
                info!(
                    "[WorkerStream] First poll: got {} events, worker_status query next",
                    events.len()
                );
            }

            // Get worker status
            let worker_status = state
                .get_worker(&worker_name_clone)
                .ok()
                .flatten()
                .map(|w| w.status.as_str().to_string());

            if !events.is_empty() {
                last_id = events.last().map(|e| e.id);

                let responses: Vec<WorkerEventResponse> = events
                    .into_iter()
                    .map(|e| WorkerEventResponse {
                        id: e.id,
                        worker_name: e.worker_name,
                        event_type: e.event_type.as_str().to_string(),
                        timestamp: e.timestamp,
                        content: e.content,
                        tool_call_id: e.tool_call_id,
                        tool_title: e.tool_title,
                        tool_kind: e.tool_kind,
                        tool_status: e.tool_status.map(|s| s.as_str().to_string()),
                        tool_input: e.tool_input,
                        tool_output: e.tool_output,
                    })
                    .collect();

                if first_poll {
                    // Send all historical events as a batch
                    info!(
                        "[WorkerStream] Emitting history with {} events",
                        responses.len()
                    );
                    let event = WorkerStreamEvent::History {
                        run_name: run_name_clone.clone(),
                        worker_name: worker_name_clone.clone(),
                        events: responses,
                        worker_status: worker_status.clone(),
                    };
                    match app.emit("worker-event", &event) {
                        Ok(_) => info!("[WorkerStream] History emitted successfully"),
                        Err(e) => info!("[WorkerStream] Failed to emit history: {}", e),
                    }
                    first_poll = false;
                } else {
                    // Send individual events
                    for response in responses {
                        let event = WorkerStreamEvent::Event {
                            run_name: run_name_clone.clone(),
                            worker_name: worker_name_clone.clone(),
                            event: response,
                        };
                        if let Err(e) = app.emit("worker-event", &event) {
                            info!("[WorkerStream] Failed to emit event: {}", e);
                        }
                    }
                }
            } else if first_poll {
                // Even if no events, send empty history to indicate stream started
                info!("[WorkerStream] Emitting empty history (no events found)");
                let event = WorkerStreamEvent::History {
                    run_name: run_name_clone.clone(),
                    worker_name: worker_name_clone.clone(),
                    events: vec![],
                    worker_status: worker_status.clone(),
                };
                match app.emit("worker-event", &event) {
                    Ok(_) => info!("[WorkerStream] Empty history emitted successfully"),
                    Err(e) => info!("[WorkerStream] Failed to emit empty history: {}", e),
                }
                first_poll = false;
            }

            // Check if worker is done (not actively working)
            let is_done = worker_status
                .as_ref()
                .map(|s| !matches!(s.as_str(), "working" | "waiting" | "awaiting"))
                .unwrap_or(false);

            if is_done && !first_poll {
                // Send status update and end stream for completed workers
                let status_event = WorkerStreamEvent::Status {
                    run_name: run_name_clone.clone(),
                    worker_name: worker_name_clone.clone(),
                    worker_status,
                };
                let _ = app.emit("worker-event", &status_event);
                break;
            }

            tokio::time::sleep(poll_interval).await;
        }

        // Clean up and notify stream ended
        stream_manager_clone.remove(&run_name_clone, &worker_name_clone);
        let end_event = WorkerStreamEvent::Ended {
            run_name: run_name_clone,
            worker_name: worker_name_clone,
        };
        let _ = app.emit("worker-event", &end_event);
    });

    Ok(())
}

/// Stop streaming worker events
#[tauri::command]
pub async fn stop_worker_event_stream(
    stream_manager: tauri::State<'_, std::sync::Arc<WorkerEventStreamManager>>,
    run_name: String,
    worker_name: String,
) -> Result<(), String> {
    stream_manager.stop(&run_name, &worker_name);
    Ok(())
}

// =============================================================================
// Config Commands
// =============================================================================

/// Get application configuration
#[tauri::command]
pub async fn get_config() -> Result<ConfigResponse, String> {
    let (cfg, _warnings) =
        config::Config::load().map_err(|e| format!("Failed to load config: {}", e))?;

    let runs_dir = cfg.runs_dir().to_string_lossy().to_string();

    Ok(ConfigResponse {
        runs_dir,
        agent_command: cfg.agent.command,
        eval_timeout: cfg.eval_timeout,
        auto_learn: cfg.auto_learn,
        max_iterations: cfg.max_iterations,
        user_message_pause: cfg.user_message_pause,
        human_in_the_loop: cfg.human_in_the_loop,
        compaction_enabled: cfg.compaction_enabled,
        compaction_threshold: cfg.compaction_threshold,
        compaction_keep_messages: cfg.compaction_keep_messages,
        auto_improve: cfg.auto_improve,
        context_warning_threshold: cfg.context_warning_threshold,
        coordinator_port: cfg.coordinator_port,
        auth: cfg.auth.into(),
        remotes: cfg
            .remotes
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect(),
        default_remote: cfg.default_remote,
        runners: cfg
            .runners
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect(),
        default_runner: cfg.default_runner,
        worker_runners: cfg.worker_runners,
    })
}

/// Save application configuration
#[tauri::command]
pub async fn save_config(updates: ConfigUpdateRequest) -> Result<(), String> {
    let config_path = config::hirsel_dir().join("config.toml");

    // Load existing config or create default
    let (mut cfg, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Apply updates
    if let Some(cmd) = updates.agent_command {
        cfg.agent.command = cmd;
    }
    if let Some(timeout) = updates.eval_timeout {
        cfg.eval_timeout = timeout;
    }
    if let Some(auto) = updates.auto_learn {
        cfg.auto_learn = auto;
    }
    if let Some(max) = updates.max_iterations {
        cfg.max_iterations = max;
    }
    if let Some(pause) = updates.user_message_pause {
        cfg.user_message_pause = pause;
    }
    if let Some(hitl) = updates.human_in_the_loop {
        cfg.human_in_the_loop = hitl;
    }
    if let Some(enabled) = updates.compaction_enabled {
        cfg.compaction_enabled = enabled;
    }
    if let Some(threshold) = updates.compaction_threshold {
        cfg.compaction_threshold = threshold;
    }
    if let Some(keep) = updates.compaction_keep_messages {
        cfg.compaction_keep_messages = keep;
    }
    if let Some(auto) = updates.auto_improve {
        cfg.auto_improve = auto;
    }
    if let Some(warning) = updates.context_warning_threshold {
        cfg.context_warning_threshold = warning;
    }
    if let Some(port) = updates.coordinator_port {
        cfg.coordinator_port = port;
    }

    // Apply auth updates
    if let Some(auth_update) = updates.auth {
        if let Some(method) = auth_update.default_method {
            cfg.auth.default_method = method.into();
        }
        if let Some(claude) = auth_update.claude {
            cfg.auth.claude = Some(claude.into());
        }
        if let Some(gemini) = auth_update.gemini {
            cfg.auth.gemini = Some(gemini.into());
        }
        if let Some(codex) = auth_update.codex {
            cfg.auth.codex = Some(codex.into());
        }
        if let Some(goose) = auth_update.goose {
            cfg.auth.goose = Some(goose.into());
        }
    }

    // Apply remotes updates (replace entire map if provided)
    if let Some(remotes) = updates.remotes {
        cfg.remotes = remotes.into_iter().map(|(k, v)| (k, v.into())).collect();
    }

    // Apply default_remote update
    if let Some(default_remote) = updates.default_remote {
        cfg.default_remote = default_remote;
    }

    // Apply runners updates (replace entire map if provided)
    if let Some(runners) = updates.runners {
        cfg.runners = runners.into_iter().map(|(k, v)| (k, v.into())).collect();
    }

    // Apply default_runner update
    if let Some(default_runner) = updates.default_runner {
        cfg.default_runner = default_runner;
    }

    // Apply worker_runners update
    if let Some(worker_runners) = updates.worker_runners {
        cfg.worker_runners = worker_runners;
    }

    // Serialize to TOML
    let toml_str =
        toml::to_string_pretty(&cfg).map_err(|e| format!("Failed to serialize config: {}", e))?;

    // Write to file
    std::fs::write(&config_path, toml_str).map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}

// =============================================================================
// Chat Session Commands (Direct AI Chat via ACP)
// =============================================================================

use crate::core::{
    ChatEvent, ChatSessionConfig, ChatSessionManager, PermissionResponse, UIContext,
};
use std::sync::Arc;

/// Start a new direct chat session with an AI agent
///
/// Returns the session ID. Events will be emitted via Tauri events.
#[tauri::command]
pub async fn start_chat_session(
    app: tauri::AppHandle,
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    agent_command: Vec<String>,
    working_dir: Option<String>,
    run_name: Option<String>,
    system_prompt: Option<String>,
) -> Result<String, String> {
    use tauri::Emitter;

    let config = ChatSessionConfig {
        agent_command,
        working_dir,
        run_name,
        system_prompt,
    };

    let (session_id, mut event_rx) = chat_manager
        .start_session(config)
        .await
        .map_err(|e| format!("Failed to start chat session: {}", e))?;

    // Spawn task to forward events to frontend
    let session_id_clone = session_id.clone();
    let app_clone = app.clone();
    eprintln!(
        "[FORWARD] Starting event forwarder for session {}",
        session_id
    );
    tokio::spawn(async move {
        eprintln!("[FORWARD] Event forwarder task started");
        while let Some(event) = event_rx.recv().await {
            eprintln!("[FORWARD] Received event: {:?}", event);
            // Emit event to frontend
            match app_clone.emit("chat-event", &event) {
                Ok(_) => eprintln!("[FORWARD] Emitted to frontend"),
                Err(e) => eprintln!("[FORWARD] Emit error: {:?}", e),
            }

            // Check if session ended
            if matches!(event, ChatEvent::SessionEnded { .. }) {
                break;
            }
        }
        eprintln!("[FORWARD] Event forwarder stopped for {}", session_id_clone);
    });

    Ok(session_id)
}

/// Send a message to an active chat session
///
/// The message will be prefixed with UI context (invisible to user).
#[tauri::command]
pub async fn send_chat_message(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    session_id: String,
    content: String,
    context: Option<UIContext>,
) -> Result<(), String> {
    chat_manager
        .send_message(&session_id, content, context)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))
}

/// Respond to a permission request from a chat session
#[tauri::command]
pub async fn respond_chat_permission(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    session_id: String,
    request_id: String,
    option_id: String,
) -> Result<(), String> {
    let response = PermissionResponse {
        request_id,
        option_id,
    };

    chat_manager
        .respond_to_permission(&session_id, response)
        .await
        .map_err(|e| format!("Failed to respond to permission: {}", e))
}

/// Stop an active chat session
#[tauri::command]
pub async fn stop_chat_session(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    session_id: String,
) -> Result<(), String> {
    chat_manager
        .stop_session(&session_id)
        .await
        .map_err(|e| format!("Failed to stop session: {}", e))
}

/// List active chat sessions
#[tauri::command]
pub async fn list_chat_sessions(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
) -> Result<Vec<String>, String> {
    Ok(chat_manager.list_sessions().await)
}

// =============================================================================
// Frontend Logging (dev mode)
// =============================================================================

/// Log a message from the frontend to the backend log file
/// This allows debugging frontend issues by checking the same log file
#[tauri::command]
pub async fn log_frontend(level: String, message: String) {
    match level.as_str() {
        "ERROR" => tracing::error!("[Frontend] {}", message),
        "WARN" => tracing::warn!("[Frontend] {}", message),
        "DEBUG" => tracing::debug!("[Frontend] {}", message),
        _ => tracing::info!("[Frontend] {}", message),
    }
}

// =============================================================================
// Debug Commands (dev mode only)
// =============================================================================

/// Count claude and acp related processes (for debug panel)
#[tauri::command]
pub async fn get_process_counts() -> Result<serde_json::Value, String> {
    #[cfg(unix)]
    {
        use std::process::Command;

        // Count claude processes
        let claude_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[c]laude' | wc -l")
            .output()
            .map_err(|e| e.to_string())?;
        let claude_count: i32 = String::from_utf8_lossy(&claude_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Count acp processes
        let acp_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[a]cp|[c]laude-code-acp' | wc -l")
            .output()
            .map_err(|e| e.to_string())?;
        let acp_count: i32 = String::from_utf8_lossy(&acp_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Count node processes
        let node_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[n]ode' | wc -l")
            .output()
            .map_err(|e| e.to_string())?;
        let node_count: i32 = String::from_utf8_lossy(&node_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Get detailed process list
        let detail_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E 'claude|acp' | grep -v grep | head -20")
            .output()
            .map_err(|e| e.to_string())?;
        let details = String::from_utf8_lossy(&detail_output.stdout).to_string();

        Ok(serde_json::json!({
            "claude": claude_count,
            "acp": acp_count,
            "node": node_count,
            "details": details
        }))
    }

    #[cfg(not(unix))]
    {
        Ok(serde_json::json!({
            "claude": 0,
            "acp": 0,
            "node": 0,
            "details": "Process counting not supported on this platform"
        }))
    }
}

/// Kill orphaned claude-code-acp processes (debug panel utility)
#[tauri::command]
pub async fn kill_orphaned_acp_processes() -> Result<serde_json::Value, String> {
    #[cfg(unix)]
    {
        use std::process::Command;

        // Use pkill to kill claude-code-acp processes
        let output = Command::new("pkill")
            .arg("-f")
            .arg("claude-code-acp")
            .output()
            .map_err(|e| e.to_string())?;

        // pkill returns 0 if processes were killed, 1 if none found
        let killed = if output.status.success() {
            // Count how many we killed by checking process count before/after
            // For simplicity, just report that some were killed
            1
        } else {
            0
        };

        Ok(serde_json::json!({ "killed": killed }))
    }

    #[cfg(not(unix))]
    {
        Ok(serde_json::json!({ "killed": 0 }))
    }
}

// =============================================================================
// Handler Registration
// =============================================================================

/// Generate the Tauri invoke handler with all commands
pub fn get_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // Run commands
        get_runs,
        get_run_detail,
        pause_run,
        resume_run,
        delete_run,
        deliver_run,
        // Draft commands
        validate_repo,
        init_project_repo,
        create_draft,
        clone_run,
        update_draft,
        start_draft,
        // Spec/Eval file commands
        read_spec_file,
        write_spec_file,
        read_eval_file,
        write_eval_file,
        // Asset commands
        save_asset,
        import_asset_from_path,
        open_assets_folder,
        get_assets_path,
        // Task commands
        get_tasks,
        add_task,
        delete_task,
        complete_task,
        unclaim_task,
        reopen_task,
        // Worker commands
        get_workers,
        attach_worker,
        open_worker_terminal,
        detach_worker,
        restart_worker,
        // Worker log commands
        get_worker_log,
        get_worker_log_path,
        parse_worker_log,
        get_eval_log,
        get_eval_log_by_path,
        // Worker events commands (ACP-based streaming)
        get_worker_events,
        clear_worker_events,
        start_worker_event_stream,
        stop_worker_event_stream,
        // Message commands
        get_messages,
        get_threads,
        get_all_unread_notifications,
        send_message,
        mark_messages_read,
        // History commands
        get_history,
        // Eval commands
        get_eval_spec,
        get_evals,
        // Config commands
        get_config,
        save_config,
        // Chat session commands
        start_chat_session,
        send_chat_message,
        respond_chat_permission,
        stop_chat_session,
        list_chat_sessions,
        // Frontend logging (dev mode)
        log_frontend,
        // Debug commands
        get_process_counts,
        kill_orphaned_acp_processes,
    ]
}
