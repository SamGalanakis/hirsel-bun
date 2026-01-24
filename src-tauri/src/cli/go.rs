//! Implementation of the `hirsel go` command - start a new run.
//!
//! This command initializes a new hirsel run with the specified spec,
//! sets up the workspace, and spawns worker processes via the Orchestrator.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::GoArgs;
use crate::core::chats::ChatError;
use crate::core::config::Config;
use crate::core::git::{get_repo_root, GitError};
#[cfg(test)]
use crate::core::names::get_available_name;
#[cfg(test)]
use crate::core::names::get_available_names;
use crate::core::names::slugify;
use crate::core::runner::RunnerError;
use crate::core::state::StateError;
#[cfg(feature = "server")]
use crate::core::tunnel::TunnelError;
use crate::core::workers::WorkerError;
use tracing::info;

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug)]
pub enum GoError {
    Io(std::io::Error),
    Git(GitError),
    State(StateError),
    Chat(ChatError),
    Worker(WorkerError),
    Tunnel(TunnelError),
    Runner(RunnerError),
    Orchestrator(String),
    InvalidSpec(String),
    RunExists(String),
    InvalidTimeLimit(String),
    InvalidWorkerScale(String),
    InvalidPauseMode(String),
    InvalidProject(String),
    InvalidRunner(String),
    InvalidProfile(String),
    NoGitRepo,
    UserAborted,
    CoordinatorError(String),
}

impl std::fmt::Display for GoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoError::Io(e) => write!(f, "IO error: {}", e),
            GoError::Git(e) => write!(f, "Git error: {}", e),
            GoError::State(e) => write!(f, "State error: {}", e),
            GoError::Chat(e) => write!(f, "Chat error: {}", e),
            GoError::Worker(e) => write!(f, "Worker error: {}", e),
            GoError::Tunnel(e) => write!(f, "Tunnel error: {}", e),
            GoError::Runner(e) => write!(f, "Runner error: {}", e),
            GoError::Orchestrator(msg) => write!(f, "Orchestrator error: {}", msg),
            GoError::InvalidSpec(msg) => write!(f, "Invalid spec: {}", msg),
            GoError::RunExists(name) => write!(f, "Run '{}' already exists and is active", name),
            GoError::InvalidTimeLimit(msg) => write!(f, "Invalid time limit: {}", msg),
            GoError::InvalidWorkerScale(msg) => write!(f, "Invalid worker scale: {}", msg),
            GoError::InvalidPauseMode(msg) => write!(f, "Invalid pause mode: {}", msg),
            GoError::InvalidProject(msg) => write!(f, "Invalid project: {}", msg),
            GoError::InvalidRunner(msg) => write!(f, "Invalid runner: {}", msg),
            GoError::InvalidProfile(msg) => write!(f, "Invalid profile: {}", msg),
            GoError::NoGitRepo => write!(f, "Not in a git repository"),
            GoError::UserAborted => write!(f, "Aborted by user"),
            GoError::CoordinatorError(msg) => write!(f, "Coordinator error: {}", msg),
        }
    }
}

impl std::error::Error for GoError {}

impl From<std::io::Error> for GoError {
    fn from(e: std::io::Error) -> Self {
        GoError::Io(e)
    }
}

impl From<GitError> for GoError {
    fn from(e: GitError) -> Self {
        GoError::Git(e)
    }
}

impl From<StateError> for GoError {
    fn from(e: StateError) -> Self {
        GoError::State(e)
    }
}

impl From<ChatError> for GoError {
    fn from(e: ChatError) -> Self {
        GoError::Chat(e)
    }
}

impl From<WorkerError> for GoError {
    fn from(e: WorkerError) -> Self {
        GoError::Worker(e)
    }
}

impl From<TunnelError> for GoError {
    fn from(e: TunnelError) -> Self {
        GoError::Tunnel(e)
    }
}

impl From<RunnerError> for GoError {
    fn from(e: RunnerError) -> Self {
        GoError::Runner(e)
    }
}

impl From<crate::core::ops::OpsError> for GoError {
    fn from(e: crate::core::ops::OpsError) -> Self {
        GoError::InvalidSpec(e.to_string())
    }
}

impl From<crate::core::orchestrator::OrchestratorError> for GoError {
    fn from(e: crate::core::orchestrator::OrchestratorError) -> Self {
        GoError::Orchestrator(e.to_string())
    }
}

pub type GoResult<T> = Result<T, GoError>;

// =============================================================================
// Worker Scale
// =============================================================================

/// Worker scale configuration - max workers to autoscale to.
/// Always starts with 1 worker and autoscales up to max.
#[derive(Debug, Clone)]
pub struct WorkerScale {
    pub max: u32,
}

impl WorkerScale {
    /// Parse worker scale from string - just the max worker count.
    /// - "4" -> autoscale up to 4 workers
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();

        // Simple number = max workers
        let max: u32 = s
            .parse()
            .map_err(|_| format!("Invalid worker count: '{}'. Use a number like '4'", s))?;
        if max == 0 {
            return Err("Worker count must be at least 1".to_string());
        }
        Ok(WorkerScale { max })
    }

    /// Initial worker count - always 1, we autoscale from there
    pub fn initial_count(&self) -> u32 {
        1
    }
}

impl std::fmt::Display for WorkerScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.max)
    }
}

// =============================================================================
// Time Limit Parsing
// =============================================================================

/// Parse time limit string into minutes
/// Supports: "30" (minutes), "30m", "1h", "1h30m", "1.5h"
pub fn parse_time_limit(s: &str) -> Result<i64, String> {
    let s = s.trim().to_lowercase();

    // Check for combined format like "1h30m"
    if s.contains('h') && s.contains('m') {
        let h_pos = s.find('h').unwrap();
        let m_pos = s.find('m').unwrap();

        if h_pos < m_pos {
            let hours_str = &s[..h_pos];
            let minutes_str = &s[h_pos + 1..m_pos];

            let hours: f64 = hours_str
                .parse()
                .map_err(|_| format!("Invalid hours: {}", hours_str))?;
            let minutes: f64 = minutes_str
                .parse()
                .map_err(|_| format!("Invalid minutes: {}", minutes_str))?;

            return Ok((hours * 60.0 + minutes) as i64);
        }
    }

    // Check for hours format
    if s.ends_with('h') {
        let num_str = &s[..s.len() - 1];
        let hours: f64 = num_str
            .parse()
            .map_err(|_| format!("Invalid hours: {}", num_str))?;
        return Ok((hours * 60.0) as i64);
    }

    // Check for minutes format
    if s.ends_with('m') {
        let num_str = &s[..s.len() - 1];
        let minutes: f64 = num_str
            .parse()
            .map_err(|_| format!("Invalid minutes: {}", num_str))?;
        return Ok(minutes as i64);
    }

    // Plain number - assume minutes
    let minutes: f64 = s
        .parse()
        .map_err(|_| format!("Invalid time limit: {}", s))?;
    Ok(minutes as i64)
}

// =============================================================================
// Run Configuration
// =============================================================================

/// Get the hirsel data directory
pub fn get_hirsel_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".hirsel"))
        .unwrap_or_else(|| PathBuf::from(".hirsel"))
}

/// Get the run directory for a given run name
pub fn get_run_dir(run_name: &str) -> PathBuf {
    get_hirsel_dir().join("runs").join(run_name)
}

/// Get the staging directory for a given run name
pub fn get_staging_dir(run_name: &str) -> PathBuf {
    get_hirsel_dir().join("staging").join(run_name)
}

/// Get the database path for a run
pub fn get_db_path(run_name: &str) -> PathBuf {
    get_run_dir(run_name).join("hirsel.db")
}

// =============================================================================
// Spec Resolution
// =============================================================================

/// Resolve spec content from file path or inline content
pub fn resolve_content(spec: &str) -> GoResult<String> {
    let path = Path::new(spec);

    // If it's a file path that exists, read it
    if path.exists() {
        return Ok(fs::read_to_string(path)?);
    }

    // Check if it's a template name
    let template_dir = get_hirsel_dir().join("templates").join(spec);
    if template_dir.exists() {
        let spec_file = template_dir.join("spec.md");
        if spec_file.exists() {
            return Ok(fs::read_to_string(spec_file)?);
        }
        return Err(GoError::InvalidSpec(format!(
            "Template '{}' has no spec.md",
            spec
        )));
    }

    // Treat as inline content if it looks like markdown
    if spec.contains('\n') || spec.starts_with('#') || spec.starts_with('-') {
        return Ok(spec.to_string());
    }

    Err(GoError::InvalidSpec(format!(
        "Spec file not found: {}",
        spec
    )))
}

// =============================================================================
// Project Setup Helpers
// =============================================================================

/// Prompt user for confirmation (returns true if user confirms)
fn prompt_confirm(message: &str, yolo: bool) -> bool {
    if yolo {
        return true;
    }

    print!("{} [y/N] ", message);
    io::stdout().flush().ok();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }

    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Initialize a git repository in the given directory
/// Initialize a git repository - delegated to ops module
fn init_git_repo(path: &Path) -> GoResult<()> {
    use crate::core::ops::init_git_repo as ops_init_git_repo;
    ops_init_git_repo(path).map_err(|e| GoError::Git(GitError::Other(e.to_string())))
}

/// Check if a path IS a git repository root (has .git directory)
/// This is different from checking if it's inside a git repo
fn is_git_repo_root(path: &Path) -> bool {
    crate::core::ops::is_git_repo_root(path)
}

/// Ensure project directory exists and is a git repo
/// Returns the canonical project path
fn ensure_project_ready(path: &Path, yolo: bool) -> GoResult<PathBuf> {
    let path_str = path.display().to_string();

    // Check if directory exists
    if !path.exists() {
        eprintln!("\n⚠️  Directory does not exist: {}\n", path_str);
        eprintln!("   This will create a new empty project directory.");

        if !prompt_confirm("Create directory?", yolo) {
            return Err(GoError::UserAborted);
        }

        fs::create_dir_all(path)?;
        eprintln!("   ✓ Created directory: {}", path_str);
    }

    // Canonicalize the path now that it exists
    let canonical_path = path.canonicalize()?;

    // Check if this directory itself is a git repo root (has .git)
    // NOT just if it's inside another git repo
    if !is_git_repo_root(&canonical_path) {
        eprintln!("\n⚠️  Not a git repository: {}\n", canonical_path.display());
        eprintln!("   Hirsel requires a git repository to track changes.");
        eprintln!("   This will initialize a new git repo with 'main' branch.");

        if !prompt_confirm("Initialize git repository?", yolo) {
            return Err(GoError::UserAborted);
        }

        init_git_repo(&canonical_path)?;
        eprintln!("   ✓ Initialized git repository");
    }

    // Return the canonical path - this IS the git root now
    Ok(canonical_path)
}

// =============================================================================
// Go Command Execution
// =============================================================================

/// Result of a successful `go` command
#[derive(Debug, serde::Serialize)]
pub struct GoOutput {
    pub run_name: String,
    pub project_path: PathBuf,
    pub run_dir: PathBuf,
    pub worker_names: Vec<String>,
    pub worker_count: u32,
    pub time_limit_minutes: Option<i64>,
}

/// Execute `hirsel go` in remote mode - delegates run creation to server
fn run_remote(
    args: &GoArgs,
    run_name: &str,
    spec_content: &str,
    eval_path: Option<&Path>,
    scale: WorkerScale,
    time_limit_minutes: Option<i64>,
    profile_name: &str,
    profile: &crate::core::config::OrchestratorProfile,
) -> GoResult<GoOutput> {
    use crate::core::credentials::CredentialStore;
    use crate::core::draft::StartingPoint;
    use crate::core::orchestrator::{
        Orchestrator, RemoteOrchestrator, StartRunRequest, TailscaleOAuth,
    };

    info!(
        "Running in remote mode (profile: {}, url: {:?})",
        profile_name, profile.url
    );

    let url = profile
        .url
        .as_ref()
        .ok_or_else(|| GoError::InvalidProfile("Remote profile missing 'url' field".to_string()))?;

    // Try credential store first, fall back to config
    let api_key = {
        let cred_key = format!("profile_{}_api_key", profile_name);
        CredentialStore::open()
            .ok()
            .and_then(|store| store.load(&cred_key).ok())
            .or_else(|| profile.api_key.clone())
    }
    .ok_or_else(|| GoError::InvalidProfile("Remote profile missing 'api_key' field".to_string()))?;

    // Get project path
    let project_path = if let Some(ref proj) = args.project {
        let path = Path::new(proj);
        ensure_project_ready(path, args.yolo)?
    } else {
        get_repo_root(Some(Path::new("."))).map_err(|_| GoError::NoGitRepo)?
    };

    // Print info
    if !args.yolo {
        println!("Creating remote run '{}' on {}", run_name, url);
        println!("  Project: {}", project_path.display());
        println!("  Workers: {} (autoscale)", scale.max);
        if let Some(limit) = time_limit_minutes {
            println!("  Time limit: {} minutes", limit);
        }
        println!();
        if !prompt_confirm("Continue?", args.yolo) {
            return Err(GoError::UserAborted);
        }
    }

    // Read eval content if specified
    let eval_content = eval_path.map(fs::read_to_string).transpose()?;

    // Build Tailscale OAuth if configured
    let tailscale_oauth = profile
        .tailscale_oauth()
        .map(|(client_id, client_secret, tag)| TailscaleOAuth {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            tag: tag.map(String::from),
        });

    // Build StartRunRequest
    let request = StartRunRequest {
        name: run_name.to_string(),
        spec: spec_content.to_string(),
        starting_point: StartingPoint::LocalFolder {
            path: project_path.to_string_lossy().to_string(),
        },
        eval: eval_content,
        worker_scale: Some(scale.max),
        time_limit_minutes,
        max_iterations: args.max_iterations,
        human_in_the_loop: if args.yolo { Some(false) } else { None },
        runner: args.runner.clone(),
        worker_runners: None,
        tailscale_oauth,
        draft: args.draft,
    };

    // Create remote orchestrator and start run
    let orchestrator = RemoteOrchestrator::new(url.clone(), api_key);
    let rt = tokio::runtime::Runtime::new().map_err(GoError::Io)?;

    info!("Starting run on remote server...");
    let detail = rt
        .block_on(orchestrator.start_run(request))
        .map_err(|e| GoError::Orchestrator(format!("Failed to start run: {}", e)))?;

    info!("Run started: {}", detail.name);

    Ok(GoOutput {
        run_name: detail.name,
        project_path,
        run_dir: get_run_dir(&run_name), // Local path for reference
        worker_names: vec![],            // Workers are on remote
        worker_count: detail.workers_total,
        time_limit_minutes,
    })
}

/// Execute the `hirsel go` command
pub fn run(args: &GoArgs) -> GoResult<GoOutput> {
    use crate::core::draft::StartingPoint;
    use crate::core::orchestrator::{create_local_orchestrator, Orchestrator, StartRunRequest};

    // Slugify run name
    let run_name = slugify(&args.run_name);
    if run_name.len() > 50 {
        return Err(GoError::InvalidSpec(format!(
            "Run name too long (max 50 chars): {}...",
            &run_name[..50]
        )));
    }

    // Parse worker scale
    let scale = WorkerScale::parse(&args.workers).map_err(GoError::InvalidWorkerScale)?;

    // Parse time limit if specified
    let time_limit_minutes = if let Some(ref tl) = args.time_limit {
        Some(parse_time_limit(tl).map_err(GoError::InvalidTimeLimit)?)
    } else {
        None
    };

    // Resolve spec content - handle --template flag
    let (spec_content, eval_path) = if let Some(ref template_name) = args.template {
        let template_dir = get_hirsel_dir().join("templates").join(template_name);
        if !template_dir.exists() {
            return Err(GoError::InvalidSpec(format!(
                "Template '{}' not found. Use 'hirsel templates' to list available templates.",
                template_name
            )));
        }
        let spec_file = template_dir.join("spec.md");
        if !spec_file.exists() {
            return Err(GoError::InvalidSpec(format!(
                "Template '{}' has no spec.md",
                template_name
            )));
        }
        let content = fs::read_to_string(&spec_file)?;
        let eval = template_dir.join("eval.md");
        let eval_path = if eval.exists() { Some(eval) } else { None };
        (content, eval_path)
    } else {
        let content = resolve_content(&args.spec)?;
        let eval_path = args.eval.as_ref().map(PathBuf::from);
        (content, eval_path)
    };

    if spec_content.trim().is_empty() {
        return Err(GoError::InvalidSpec("Spec is empty".to_string()));
    }

    // Read eval content if specified
    let eval_content = eval_path.as_ref().map(fs::read_to_string).transpose()?;

    // Check if using a remote profile - delegate to run_remote
    if let Some(ref profile_name) = args.profile {
        let (config, _) =
            Config::load().map_err(|e| GoError::InvalidProfile(format!("Config error: {}", e)))?;

        let profile = config.profiles.get(profile_name).ok_or_else(|| {
            GoError::InvalidProfile(format!("Profile '{}' not found", profile_name))
        })?;

        if matches!(profile.mode, crate::core::config::OrchestratorMode::Remote) {
            return run_remote(
                args,
                &run_name,
                &spec_content,
                eval_path.as_deref(),
                scale,
                time_limit_minutes,
                profile_name,
                profile,
            );
        }
    }

    // Get project path (specified, or detect from current directory)
    let project_path = if let Some(ref proj) = args.project {
        let path = Path::new(proj);
        ensure_project_ready(path, args.yolo)?
    } else {
        get_repo_root(Some(Path::new("."))).map_err(|_| GoError::NoGitRepo)?
    };

    // Build StartRunRequest and use orchestrator
    let orchestrator =
        create_local_orchestrator().map_err(|e| GoError::Orchestrator(e.to_string()))?;

    let request = StartRunRequest {
        name: run_name.clone(),
        spec: spec_content,
        starting_point: StartingPoint::LocalFolder {
            path: project_path.to_string_lossy().to_string(),
        },
        eval: eval_content,
        worker_scale: Some(scale.max),
        time_limit_minutes,
        max_iterations: args.max_iterations,
        human_in_the_loop: if args.yolo { Some(false) } else { None },
        runner: args.runner.clone(),
        worker_runners: None,
        tailscale_oauth: None,
        draft: args.draft,
    };

    let rt = tokio::runtime::Runtime::new().map_err(GoError::Io)?;
    let detail = rt
        .block_on(orchestrator.start_run(request))
        .map_err(|e| GoError::Orchestrator(e.to_string()))?;

    Ok(GoOutput {
        run_name: detail.name,
        project_path,
        run_dir: get_run_dir(&run_name),
        worker_names: vec![],
        worker_count: detail.workers_total,
        time_limit_minutes,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_worker_scale_simple() {
        let scale = WorkerScale::parse("3").unwrap();
        assert_eq!(scale.max, 3);
        assert_eq!(scale.initial_count(), 1);
    }

    #[test]
    fn test_parse_worker_scale_invalid() {
        assert!(WorkerScale::parse("0").is_err());
        assert!(WorkerScale::parse("abc").is_err());
        assert!(WorkerScale::parse("1-5").is_err()); // Legacy format not supported
        assert!(WorkerScale::parse("2+").is_err()); // Legacy format not supported
    }

    #[test]
    fn test_parse_time_limit_minutes() {
        assert_eq!(parse_time_limit("30").unwrap(), 30);
        assert_eq!(parse_time_limit("30m").unwrap(), 30);
        assert_eq!(parse_time_limit("45m").unwrap(), 45);
    }

    #[test]
    fn test_parse_time_limit_hours() {
        assert_eq!(parse_time_limit("1h").unwrap(), 60);
        assert_eq!(parse_time_limit("2h").unwrap(), 120);
        assert_eq!(parse_time_limit("1.5h").unwrap(), 90);
    }

    #[test]
    fn test_parse_time_limit_combined() {
        assert_eq!(parse_time_limit("1h30m").unwrap(), 90);
        assert_eq!(parse_time_limit("2h15m").unwrap(), 135);
    }

    #[test]
    fn test_get_available_name() {
        let used = vec!["bonnie-cheviot".to_string(), "braw-merino".to_string()];
        let name = get_available_name(&used);
        // Should return a name not in the used list
        assert!(!used.contains(&name));
        // Should have adjective-breed format
        assert!(name.contains('-'));
    }

    #[test]
    fn test_get_available_name_avoids_used() {
        // Generate some names and make sure new ones don't collide
        let mut used = Vec::new();
        for _ in 0..10 {
            let name = get_available_name(&used);
            assert!(!used.contains(&name), "Name {} was already used", name);
            used.push(name);
        }
    }

    #[test]
    fn test_get_available_names() {
        let names = get_available_names(5, &[]);
        assert_eq!(names.len(), 5);
        // All names should be unique
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(name.clone()), "Duplicate name: {}", name);
            assert!(
                name.contains('-'),
                "Name should have adjective-breed format: {}",
                name
            );
        }
    }

    #[test]
    fn test_get_available_names_avoids_used() {
        let used = vec!["bonnie-cheviot".to_string(), "misty-gotland".to_string()];
        let names = get_available_names(3, &used);
        assert_eq!(names.len(), 3);
        // None should be in the used list
        for name in &names {
            assert!(!used.contains(name), "Name {} was in used list", name);
        }
    }
}
