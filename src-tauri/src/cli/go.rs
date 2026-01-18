//! Implementation of the `hirsel go` command - start a new run.
//!
//! This command initializes a new hirsel run with the specified spec,
//! sets up git worktrees for workers, and spawns the worker processes.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::config::get_agent_command;
use crate::cli::GoArgs;
use crate::core::chats::ChatError;
use crate::core::config::Config;
use crate::core::coordinator_api::CoordinatorServer;
use crate::core::files::Files;
use crate::core::git::{get_repo_root, GitError};
use crate::core::names;
use crate::core::ops::{
    register_workers, setup_run_workspace, spawn_local_workers, RunSetupConfig, SpawnWorkersConfig,
};
use crate::core::remote::{parse_remote_spec, RemoteConfig, RemoteError, RemoteWorkerSpawner};
use crate::core::runner::{self, Runner, RunnerConfig, RunnerError};
use crate::core::state::{SQLiteState, StateError, Status};
use crate::core::tunnel::{TunnelError, TunnelManager};
use crate::core::workers::WorkerError;
use tracing::{info, warn};

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
    Remote(RemoteError),
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
            GoError::Remote(e) => write!(f, "Remote error: {}", e),
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

impl From<RemoteError> for GoError {
    fn from(e: RemoteError) -> Self {
        GoError::Remote(e)
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
// Worker Names
// =============================================================================

/// Maximum attempts to generate a unique name before falling back
const MAX_NAME_ATTEMPTS: usize = 100;

/// Get an available worker name that's not in use.
/// Uses sheep breed names (adjective-breed format) for the hirsel theme.
pub fn get_available_name(used: &[String]) -> String {
    // Try generating random names until we find one not in use
    for _ in 0..MAX_NAME_ATTEMPTS {
        let name = names::generate_worker_name();
        if !used.iter().any(|u| u == &name) {
            return name;
        }
    }
    // Fallback: generate a numbered name
    for i in 1.. {
        let name = format!("worker-{}", i);
        if !used.iter().any(|u| u == &name) {
            return name;
        }
    }
    unreachable!()
}

/// Get multiple available worker names.
/// Ensures all returned names are unique and not in the used list.
pub fn get_available_names(count: u32, used: &[String]) -> Vec<String> {
    let mut result = Vec::with_capacity(count as usize);
    let mut all_used: std::collections::HashSet<String> = used.iter().cloned().collect();

    // First try to get unique names from the batch generator
    let candidates = names::generate_unique_names(count as usize * 2);
    for name in candidates {
        if result.len() >= count as usize {
            break;
        }
        if !all_used.contains(&name) {
            all_used.insert(name.clone());
            result.push(name);
        }
    }

    // If we still need more names, generate them one by one
    while result.len() < count as usize {
        let name = get_available_name(&all_used.iter().cloned().collect::<Vec<_>>());
        all_used.insert(name.clone());
        result.push(name);
    }

    result
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
// Slugify
// =============================================================================

/// Convert a string to a valid run name slug
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_separator = false;

    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator && !slug.is_empty() {
            slug.push('-');
            last_was_separator = true;
        }
    }

    // Remove trailing separator
    if slug.ends_with('-') {
        slug.pop();
    }

    slug
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

/// Create a tarball of a project directory, excluding common build artifacts
fn create_project_tarball(project_path: &Path) -> GoResult<Vec<u8>> {
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
            let name = e.file_name().to_str().unwrap_or("");
            // Exclude common build/cache directories and files
            !matches!(
                name,
                "node_modules"
                    | "target"
                    | ".git"
                    | ".venv"
                    | "__pycache__"
                    | ".mypy_cache"
                    | ".pytest_cache"
                    | "dist"
                    | "build"
                    | ".next"
                    | ".nuxt"
                    | "coverage"
                    | ".turbo"
                    | ".vercel"
                    | ".netlify"
            )
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let relative_path = path.strip_prefix(project_path).unwrap_or(path);

        if path == project_path {
            continue; // Skip root directory itself
        }

        if path.is_file() {
            builder
                .append_path_with_name(path, relative_path)
                .map_err(|e| GoError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        } else if path.is_dir() {
            builder
                .append_dir(relative_path, path)
                .map_err(|e| GoError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }
    }

    builder
        .into_inner()
        .map_err(|e| GoError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
        .finish()
        .map_err(|e| GoError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    info!("Created tarball: {} bytes", buffer.len());
    Ok(buffer)
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
    use crate::core::orchestrator::{CreateRunRequest, RemoteOrchestrator, TailscaleOAuth};

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

    // Create tarball of project
    info!("Creating tarball of project: {}", project_path.display());
    let tarball = create_project_tarball(&project_path)?;
    info!("Tarball size: {} bytes", tarball.len());

    // Read eval content if specified
    let eval_content = eval_path.map(|p| fs::read_to_string(p)).transpose()?;

    // Create remote orchestrator
    let orchestrator = RemoteOrchestrator::new(url.clone(), api_key);

    // Create the run
    let tailscale_oauth = profile
        .tailscale_oauth()
        .map(|(client_id, client_secret, tag)| TailscaleOAuth {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            tag: tag.map(String::from),
        });

    let create_request = CreateRunRequest {
        name: run_name.to_string(),
        spec: spec_content.to_string(),
        runner: args.runner.clone(),
        worker_scale: Some(scale.max),
        time_limit_minutes: time_limit_minutes.map(|m| m as u32),
        max_iterations: args.max_iterations.map(|m| m as u32),
        human_in_the_loop: None, // Could add a flag for this
        eval: eval_content,
        tailscale_oauth,
    };

    let rt = tokio::runtime::Runtime::new().map_err(|e| GoError::Io(e.into()))?;

    info!("Creating run on remote server...");
    let create_response = rt
        .block_on(orchestrator.create_run(create_request))
        .map_err(|e| GoError::Orchestrator(format!("Failed to create run: {}", e)))?;

    info!("Run created: {}", create_response.name);

    // Upload project files
    info!("Uploading project files...");
    rt.block_on(orchestrator.upload_files(&create_response.name, tarball))
        .map_err(|e| GoError::Orchestrator(format!("Failed to upload files: {}", e)))?;

    info!("Files uploaded");

    // Spawn initial worker (server will use autoscaling for more)
    if !args.draft {
        info!("Spawning initial worker...");
        let spawn_response = rt
            .block_on(orchestrator.spawn_workers(&create_response.name, 1))
            .map_err(|e| GoError::Orchestrator(format!("Failed to spawn workers: {}", e)))?;

        info!("Spawned workers: {:?}", spawn_response.workers);

        Ok(GoOutput {
            run_name: create_response.name,
            project_path,
            run_dir: PathBuf::from(create_response.run_dir),
            worker_names: spawn_response.workers,
            worker_count: 1,
            time_limit_minutes,
        })
    } else {
        info!("Draft mode: skipping worker spawn");
        Ok(GoOutput {
            run_name: create_response.name,
            project_path,
            run_dir: PathBuf::from(create_response.run_dir),
            worker_names: vec![],
            worker_count: 0,
            time_limit_minutes,
        })
    }
}

/// Execute the `hirsel go` command
pub fn run(args: &GoArgs) -> GoResult<GoOutput> {
    // Slugify run name
    let run_name = slugify(&args.run_name);
    if run_name.len() > 50 {
        return Err(GoError::InvalidSpec(format!(
            "Run name too long (max 50 chars): {}...",
            &run_name[..50]
        )));
    }

    // Validate pause_mode if specified
    if let Some(ref pm) = args.pause_mode {
        if pm != "sender" && pm != "all" {
            return Err(GoError::InvalidPauseMode(
                "pause_mode must be 'sender' or 'all'".to_string(),
            ));
        }
    }

    // Parse worker scale
    let scale = WorkerScale::parse(&args.workers).map_err(GoError::InvalidWorkerScale)?;

    // Parse time limit if specified
    let time_limit_minutes = if let Some(ref tl) = args.time_limit {
        Some(parse_time_limit(tl).map_err(GoError::InvalidTimeLimit)?)
    } else {
        None
    };

    // Resolve spec content early (needed for both local and remote modes)
    // Handle --template flag
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

    // Check if we're using a remote profile
    if let Some(ref profile_name) = args.profile {
        // Load config and check if this profile is remote mode
        let (config, _) =
            Config::load().map_err(|e| GoError::InvalidProfile(format!("Config error: {}", e)))?;

        let profile = config.profiles.get(profile_name).ok_or_else(|| {
            GoError::InvalidProfile(format!("Profile '{}' not found", profile_name))
        })?;

        if matches!(profile.mode, crate::core::config::OrchestratorMode::Remote) {
            // Run in remote mode - delegate to server
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

    // Get run directories
    let run_dir = get_run_dir(&run_name);
    let staging_dir = get_staging_dir(&run_name);
    let db_path = get_db_path(&run_name);

    // Check for existing run
    if run_dir.exists() {
        let state = SQLiteState::new(db_path.clone())?;
        let status = state.status()?;

        match status {
            Status::Working | Status::Eval => {
                return Err(GoError::RunExists(run_name));
            }
            _ => {
                // Old run exists but not active - remove it
                fs::remove_dir_all(&run_dir)?;
                if staging_dir.exists() {
                    fs::remove_dir_all(&staging_dir)?;
                }
            }
        }
    }

    // Get project path (specified, or detect from current directory)
    let project_path = if let Some(ref proj) = args.project {
        let path = Path::new(proj);
        // Use ensure_project_ready for specified paths - this handles:
        // - Creating the directory if it doesn't exist
        // - Initializing git if it's not a repo
        // Both with user confirmation (unless --yolo)
        ensure_project_ready(path, args.yolo)?
    } else {
        // For current directory, just require it to be a git repo
        get_repo_root(Some(Path::new("."))).map_err(|_| GoError::NoGitRepo)?
    };

    // Create staging directory with spec
    fs::create_dir_all(&staging_dir)?;
    fs::write(staging_dir.join("spec.md"), &spec_content)?;

    // Create bootstrap tasks.md
    fs::write(
        staging_dir.join("tasks.md"),
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Read spec, create exploration tasks |\n",
    )?;

    // Create tasks detail folder
    let tasks_dir = staging_dir.join("tasks");
    fs::create_dir_all(&tasks_dir)?;
    fs::write(tasks_dir.join("scope.md"), "")?;

    // Create run directory
    fs::create_dir_all(&run_dir)?;

    // Initialize Files
    let files = Files::new(run_dir.clone());
    files.init_dirs()?;

    // Copy staging files to run directory
    for filename in &["spec.md", "tasks.md"] {
        let src = staging_dir.join(filename);
        if src.exists() {
            fs::copy(&src, run_dir.join(filename))?;
        }
    }

    // Copy tasks folder
    if tasks_dir.exists() {
        let dest_tasks = run_dir.join("tasks");
        if dest_tasks.exists() {
            fs::remove_dir_all(&dest_tasks)?;
        }
        copy_dir_recursive(&tasks_dir, &dest_tasks)?;
    }

    // Parse remote specs if provided
    let remote_specs: Vec<(String, u32)> = if let Some(ref remote_str) = args.remote {
        // Support multiple remote specs separated by commas
        remote_str
            .split(',')
            .map(|s| parse_remote_spec(s.trim()))
            .collect()
    } else {
        Vec::new()
    };
    let total_remote_workers: u32 = remote_specs.iter().map(|(_, count)| *count).sum();

    // Get worker names for local workers
    let local_count = scale.initial_count();
    let local_worker_names = get_available_names(local_count, &[]);

    // Get worker names for remote workers
    let remote_worker_names = get_available_names(total_remote_workers, &local_worker_names);

    // Combine all worker names
    let mut worker_names = local_worker_names.clone();
    worker_names.extend(remote_worker_names.clone());

    // Determine if multi-worker mode (current or potential via autoscale)
    // Include both local and remote workers in the count
    let total_workers = local_count + total_remote_workers;
    let (is_multi_worker, leader) = if total_workers > 1 || scale.max > 1 {
        (true, Some(worker_names[0].clone()))
    } else {
        (false, None)
    };

    // Clean up staging
    fs::remove_dir_all(&staging_dir)?;

    // Initialize state
    let state = SQLiteState::new(db_path.clone())?;
    state.init_state(Some(project_path.to_str().unwrap_or(".")))?;
    state.set_request(Some(&spec_content))?;
    state.set_worker_scale(&scale.to_string())?;

    // Add scope task if not exists
    let _ = state.add_task("scope", "Read spec, create exploration tasks", None, None);

    // Pre-claim scope for first worker
    let first_worker = &worker_names[0];
    let _ = state.claim_task("scope", first_worker);

    // Set up workspace, worker clones, and chats using shared ops
    let setup_config = RunSetupConfig {
        run_name: run_name.clone(),
        project_path: project_path.clone(),
        run_dir: run_dir.clone(),
        worker_names: local_worker_names.clone(),
        additional_chat_workers: remote_worker_names.clone(), // Remote workers need chats but not clones
        is_multi_worker,
        leader_name: leader.clone(),
    };

    let setup_result = setup_run_workspace(&setup_config)?;
    let workspace_dir = setup_result.workspace_dir;
    let local_worker_dirs = setup_result.worker_dirs;

    // Register local workers in state
    register_workers(&state, &local_worker_dirs, "local")?;

    // Register remote workers in state (work_dir is set to remote base path)
    for worker_name in &remote_worker_names {
        state.add_worker(worker_name, "/tmp/hirsel-remote", "remote")?;
    }

    // Set run status - Draft if --draft flag, otherwise Working
    if args.draft {
        state.set_status(Status::Draft)?;
    } else {
        state.set_status(Status::Working)?;
    }

    // Set time limit if specified (but don't set started_at for drafts)
    if let Some(limit) = time_limit_minutes {
        state.set_time_limit_minutes(Some(limit))?;
        if !args.draft {
            state.set_started_at(None)?;
        }
    }

    // Set HITL mode
    if args.yolo {
        state.set_human_in_the_loop(false)?;
    }

    // Set max iterations if specified
    if let Some(max_iter) = args.max_iterations {
        state.set_max_iterations(Some(max_iter))?;
    }

    // Set pause mode if specified
    if let Some(ref pm) = args.pause_mode {
        state.set_pause_mode(pm)?;
    }

    // Store eval path if specified (from template or CLI)
    if let Some(ref eval) = eval_path {
        // Store the eval file path for later use
        let eval_dest = run_dir.join("eval.md");
        if eval.exists() {
            fs::copy(eval, &eval_dest)?;
        }
    }

    // Copy assets folder if specified
    if let Some(ref assets_path) = args.assets {
        let assets_src = PathBuf::from(assets_path);
        if !assets_src.exists() {
            return Err(GoError::InvalidSpec(format!(
                "Assets folder not found: {}",
                assets_path
            )));
        }
        if !assets_src.is_dir() {
            return Err(GoError::InvalidSpec(format!(
                "Assets path is not a directory: {}",
                assets_path
            )));
        }

        let assets_dest = run_dir.join("assets");
        fs::create_dir_all(&assets_dest)?;

        // Copy all files from assets folder
        let mut count = 0;
        for entry in fs::read_dir(&assets_src)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name() {
                    fs::copy(&path, assets_dest.join(filename))?;
                    count += 1;
                }
            }
        }
        if count > 0 {
            info!("Copied {} asset(s) from {}", count, assets_path);
        }
    }

    // Spawn worker processes (skip for draft runs)
    if !args.draft {
        let agent_command = get_agent_command();
        let spec_path = run_dir.join("spec.md");
        let teammates: Vec<String> = worker_names.clone();

        // Determine runner config
        let (hirsel_config, _config_warnings) = Config::load().unwrap_or_default();
        let runner_config = match &args.runner {
            Some(runner_name) => {
                // Try to get from config, or create default based on name
                if runner_name == "local" {
                    RunnerConfig::Local
                } else if runner_name == "sprite" || runner_name == "sprites" {
                    hirsel_config
                        .get_runner("sprites")
                        .unwrap_or_else(|| RunnerConfig::Sprite(Default::default()))
                } else {
                    hirsel_config.get_runner(runner_name).ok_or_else(|| {
                        GoError::InvalidRunner(format!(
                            "Runner '{}' not found in config. Available: {}",
                            runner_name,
                            hirsel_config.runner_names().join(", ")
                        ))
                    })?
                }
            }
            None => RunnerConfig::Local,
        };

        // Spawn workers based on runner type
        match &runner_config {
            RunnerConfig::Local => {
                // Spawn LOCAL workers using shared ops
                let spawn_config = SpawnWorkersConfig {
                    run_name: run_name.clone(),
                    run_dir: run_dir.clone(),
                    spec_path: spec_path.clone(),
                    agent_command: agent_command.clone(),
                    is_multi_worker,
                    leader_name: leader.clone(),
                    all_worker_names: teammates.clone(),
                };

                let spawn_result = spawn_local_workers(&spawn_config, &local_worker_dirs, &state);

                // Log any failures
                for (worker_name, error) in &spawn_result.failed {
                    eprintln!("Warning: Failed to spawn worker {}: {}", worker_name, error);
                }
            }
            RunnerConfig::Sprite(sprite_config) => {
                // Spawn SPRITE workers
                // Need to start coordinator first for API access
                info!("Starting coordinator for sprite workers");
                let coordinator_port = hirsel_config.coordinator_port;

                let coord_state = SQLiteState::new(db_path.clone())?;
                let mut coordinator = CoordinatorServer::new(
                    coord_state,
                    "0.0.0.0".to_string(),
                    coordinator_port,
                    run_dir.clone(),
                    run_name.clone(),
                    Some(workspace_dir.clone()),
                );

                // Start coordinator in background
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| GoError::CoordinatorError(e.to_string()))?;

                let coord_handle = std::thread::spawn(move || {
                    rt.block_on(async {
                        if let Err(e) = coordinator.start().await {
                            eprintln!("Coordinator error: {}", e);
                        }
                    });
                });

                // Give coordinator time to start
                std::thread::sleep(std::time::Duration::from_millis(500));

                // Create sprite runner
                let sprite_runner = runner::SpriteRunner::new(sprite_config.clone());

                // Spawn workers on sprites
                let spawn_rt = tokio::runtime::Runtime::new()
                    .map_err(|e| GoError::CoordinatorError(e.to_string()))?;

                for (i, worker_name) in local_worker_names.iter().enumerate() {
                    let is_leader = i == 0 && is_multi_worker;

                    // Build env vars for worker
                    let mut env_vars: HashMap<String, String> = std::env::vars()
                        .filter(|(k, _)| {
                            k.starts_with("ANTHROPIC_")
                                || k.starts_with("OPENAI_")
                                || k.starts_with("CLAUDE_")
                        })
                        .collect();
                    env_vars.insert(
                        "ACP_PERMISSION_MODE".to_string(),
                        "bypassPermissions".to_string(),
                    );

                    let spawn_config = runner::WorkerSpawnConfig {
                        run_name: run_name.clone(),
                        worker_name: worker_name.clone(),
                        work_dir: workspace_dir.clone(),
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
                        env_vars: Some(env_vars),
                        coordinator_url: Some(format!("http://localhost:{}", coordinator_port)),
                        project_url: Some(format!("http://localhost:{}/git", coordinator_port)),
                        tailscale_authkey: None, // Not needed for local coordinator
                    };

                    match spawn_rt.block_on(sprite_runner.spawn(&spawn_config)) {
                        Ok(result) => {
                            info!(
                                "Spawned sprite worker {} (sprite: {})",
                                result.handle.worker_name, result.handle.runner_id
                            );
                        }
                        Err(e) => {
                            warn!("Failed to spawn sprite worker {}: {}", worker_name, e);
                        }
                    }
                }

                // Don't wait for coordinator thread
                drop(coord_handle);
            }
            RunnerConfig::Ssh(_ssh_config) => {
                // For SSH, use the existing remote worker flow
                warn!("SSH runner specified via --runner flag. Use --remote for SSH workers.");
            }
        }

        // Spawn REMOTE workers if any remote specs were provided (independent of --runner)
        if !remote_specs.is_empty() {
            spawn_remote_workers(
                &run_name,
                &run_dir,
                &db_path,
                &workspace_dir,
                &remote_specs,
                &remote_worker_names,
                &agent_command,
                is_multi_worker,
                leader.as_deref(),
                &teammates,
            )?;
        }
    }

    // Ensure daemon is running for lifecycle management (eval triggering, time limits)
    // This is non-blocking - if daemon can't start, the run still proceeds
    if !args.draft {
        match crate::daemon::DaemonClient::connect_or_start() {
            Ok(_) => {
                tracing::debug!("Daemon is running for lifecycle management");
            }
            Err(e) => {
                warn!("Could not start daemon for lifecycle management: {}", e);
            }
        }
    }

    Ok(GoOutput {
        run_name,
        project_path,
        run_dir,
        worker_names,
        worker_count: total_workers,
        time_limit_minutes,
    })
}

/// Spawn remote workers via SSH with coordinator API and tunnels
#[allow(clippy::too_many_arguments)]
fn spawn_remote_workers(
    run_name: &str,
    run_dir: &Path,
    db_path: &Path,
    workspace_dir: &Path,
    remote_specs: &[(String, u32)],
    remote_worker_names: &[String],
    agent_command: &[String],
    is_multi_worker: bool,
    leader_name: Option<&str>,
    all_teammates: &[String],
) -> GoResult<()> {
    const COORDINATOR_PORT: u16 = 19700;
    const TUNNEL_BASE_PORT: u16 = 19800;

    info!(
        "Starting coordinator API for remote workers on port {}",
        COORDINATOR_PORT
    );

    // Create the SQLite state for the coordinator
    let state = SQLiteState::new(db_path.to_path_buf())
        .map_err(|e| GoError::CoordinatorError(format!("Failed to open database: {}", e)))?;

    // Create and start the coordinator server
    let mut coordinator = CoordinatorServer::new(
        state,
        "127.0.0.1".to_string(),
        COORDINATOR_PORT,
        run_dir.to_path_buf(),
        run_name.to_string(),
        Some(workspace_dir.to_path_buf()),
    );

    // Start the coordinator in a background thread
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| GoError::CoordinatorError(format!("Failed to create runtime: {}", e)))?;

    let coordinator_handle = std::thread::spawn(move || {
        rt.block_on(async {
            if let Err(e) = coordinator.start().await {
                eprintln!("Coordinator server error: {}", e);
            }
        });
    });

    // Give the server a moment to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Create tunnel manager
    let mut tunnel_manager = TunnelManager::new(TUNNEL_BASE_PORT);

    // Collect environment variables to forward (API keys)
    let env_vars: HashMap<String, String> = std::env::vars()
        .filter(|(k, _)| {
            k.starts_with("ANTHROPIC_")
                || k.starts_with("OPENAI_")
                || k.starts_with("CLAUDE_")
                || k == "ACP_PERMISSION_MODE"
        })
        .collect();

    // Track which remote worker name we're on
    let mut remote_worker_idx = 0;

    // Spawn remote workers for each host
    for (host, worker_count) in remote_specs {
        info!("Setting up {} remote worker(s) on {}", worker_count, host);

        // Create SSH tunnel for this host
        let tunnel = tunnel_manager.create_tunnel(host, None, 22, Some(COORDINATOR_PORT))?;
        let tunnel_port = tunnel.remote_port;

        info!(
            "Created tunnel to {} (remote port {} -> local {})",
            host, tunnel_port, COORDINATOR_PORT
        );

        // Create remote config
        let config = RemoteConfig::new(host.clone())
            .with_work_base(format!("/tmp/hirsel-remote/{}", run_name));

        // Create spawner for this host
        let spawner = RemoteWorkerSpawner::new(config, tunnel_port);

        // Git HTTP URL for cloning (via tunnel)
        let git_url = format!("http://127.0.0.1:{}/git", tunnel_port);

        // Spawn workers on this host
        for _ in 0..*worker_count {
            if remote_worker_idx >= remote_worker_names.len() {
                eprintln!("Warning: Not enough worker names for remote workers");
                break;
            }

            let worker_name = &remote_worker_names[remote_worker_idx];
            remote_worker_idx += 1;

            // First worker is leader if we're in multi-worker mode and have no local workers
            let is_leader =
                remote_worker_idx == 1 && is_multi_worker && leader_name == Some(worker_name);

            // Build teammates list (exclude self)
            let teammates: Option<Vec<String>> = if is_multi_worker {
                Some(
                    all_teammates
                        .iter()
                        .filter(|t| *t != worker_name)
                        .cloned()
                        .collect(),
                )
            } else {
                None
            };

            info!("Spawning remote worker {} on {}", worker_name, host);

            match spawner.spawn_worker(
                run_name,
                worker_name,
                &git_url,
                agent_command,
                Some(&env_vars),
                is_leader,
                leader_name,
                teammates.as_deref(),
            ) {
                Ok(pid) => {
                    info!(
                        "Remote worker {} started on {} (PID {})",
                        worker_name, host, pid
                    );
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to spawn remote worker {} on {}: {}",
                        worker_name, host, e
                    );
                }
            }
        }
    }

    // Don't wait for the coordinator thread - it will keep running
    // The coordinator will be shut down when the main process exits
    drop(coordinator_handle);

    Ok(())
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
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
    fn test_slugify() {
        assert_eq!(slugify("My Cool Run"), "my-cool-run");
        assert_eq!(slugify("test_run_123"), "test-run-123");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert_eq!(slugify("CamelCase"), "camelcase");
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
