//! Implementation of the `hirsel go` command - start a new run.
//!
//! This command initializes a new hirsel run with the specified spec,
//! sets up git worktrees for workers, and spawns the worker processes.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::config::get_agent_command;
use crate::cli::GoArgs;
use crate::core::files::Files;
use crate::core::git::{create_worker_clone, create_workspace, get_repo_root, GitError};
use crate::core::state::{SQLiteState, StateError, Status};
use crate::core::chats::{
    create_default_group_chat, create_default_user_chat, create_learnings_thread,
    create_worker_chat, ChatError,
};
use crate::core::workers::{spawn_worker, WorkerError, WorkerSpawnConfig};
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
    InvalidSpec(String),
    RunExists(String),
    InvalidTimeLimit(String),
    InvalidWorkerScale(String),
    InvalidPauseMode(String),
    InvalidProject(String),
    NoGitRepo,
}

impl std::fmt::Display for GoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoError::Io(e) => write!(f, "IO error: {}", e),
            GoError::Git(e) => write!(f, "Git error: {}", e),
            GoError::State(e) => write!(f, "State error: {}", e),
            GoError::Chat(e) => write!(f, "Chat error: {}", e),
            GoError::Worker(e) => write!(f, "Worker error: {}", e),
            GoError::InvalidSpec(msg) => write!(f, "Invalid spec: {}", msg),
            GoError::RunExists(name) => write!(f, "Run '{}' already exists and is active", name),
            GoError::InvalidTimeLimit(msg) => write!(f, "Invalid time limit: {}", msg),
            GoError::InvalidWorkerScale(msg) => write!(f, "Invalid worker scale: {}", msg),
            GoError::InvalidPauseMode(msg) => write!(f, "Invalid pause mode: {}", msg),
            GoError::InvalidProject(msg) => write!(f, "Invalid project: {}", msg),
            GoError::NoGitRepo => write!(f, "Not in a git repository"),
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

pub type GoResult<T> = Result<T, GoError>;

// =============================================================================
// Worker Scale (minimal implementation until config.rs is available)
// =============================================================================

/// Worker scale configuration - how many workers to run
#[derive(Debug, Clone)]
pub struct WorkerScale {
    pub min: u32,
    pub max: Option<u32>,
    pub autoscale: bool,
}

impl WorkerScale {
    /// Parse worker scale from string
    /// - "3" -> fixed 3 workers
    /// - "1-5" -> autoscale between 1 and 5
    /// - "2+" -> autoscale from 2 with no upper limit
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();

        // Check for "N+" pattern (autoscale from N)
        if s.ends_with('+') {
            let num_str = &s[..s.len() - 1];
            let min: u32 = num_str
                .parse()
                .map_err(|_| format!("Invalid worker count: {}", num_str))?;
            if min == 0 {
                return Err("Worker count must be at least 1".to_string());
            }
            return Ok(WorkerScale {
                min,
                max: None,
                autoscale: true,
            });
        }

        // Check for "N-M" pattern (range)
        if let Some(dash_pos) = s.find('-') {
            let min_str = &s[..dash_pos];
            let max_str = &s[dash_pos + 1..];
            let min: u32 = min_str
                .parse()
                .map_err(|_| format!("Invalid minimum worker count: {}", min_str))?;
            let max: u32 = max_str
                .parse()
                .map_err(|_| format!("Invalid maximum worker count: {}", max_str))?;
            if min == 0 {
                return Err("Minimum worker count must be at least 1".to_string());
            }
            if max < min {
                return Err(format!(
                    "Maximum ({}) must be >= minimum ({})",
                    max, min
                ));
            }
            return Ok(WorkerScale {
                min,
                max: Some(max),
                autoscale: min != max,
            });
        }

        // Simple number - fixed count
        let count: u32 = s
            .parse()
            .map_err(|_| format!("Invalid worker count: {}", s))?;
        if count == 0 {
            return Err("Worker count must be at least 1".to_string());
        }
        Ok(WorkerScale {
            min: count,
            max: Some(count),
            autoscale: false,
        })
    }

    /// Get initial worker count
    pub fn initial_count(&self) -> u32 {
        self.min
    }
}

impl std::fmt::Display for WorkerScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.max, self.autoscale) {
            (None, true) => write!(f, "{}+", self.min),
            (Some(max), true) if max != self.min => write!(f, "{}-{}", self.min, max),
            (Some(max), _) => write!(f, "{}", max),
            (None, false) => write!(f, "{}", self.min),
        }
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

/// Greek hero names for workers
const WORKER_NAMES: &[&str] = &[
    "achilles", "hector", "ajax", "odysseus", "diomedes", "patroclus", "nestor",
    "menelaus", "agamemnon", "paris", "priam", "aeneas", "sarpedon", "glaucus",
    "idomeneus", "meriones", "teucer", "antilochus", "thrasymedes", "eurypylus",
    "machaon", "podalirius", "philoctetes", "neoptolemus", "pyrrhus", "calchas",
    "automedon", "phoenix", "stentor", "polydamas", "deiphobus", "helenus",
    "cassandra", "andromache", "hecuba", "polyxena", "troilus", "lycaon",
    "pandarus", "antenor", "theano", "laocoon", "sinon", "epeus", "protesilaus",
    "palamedes", "capaneus", "amphiaraus", "tydeus", "polynices", "eteocles",
];

/// Get an available worker name that's not in use
pub fn get_available_name(used: &[String]) -> String {
    for name in WORKER_NAMES {
        if !used.iter().any(|u| u == *name) {
            return name.to_string();
        }
    }
    // Fallback: generate a numbered name
    for i in 1.. {
        let name = format!("worker_{}", i);
        if !used.iter().any(|u| u == &name) {
            return name;
        }
    }
    unreachable!()
}

/// Get multiple available worker names
pub fn get_available_names(count: u32, used: &[String]) -> Vec<String> {
    let mut names = Vec::with_capacity(count as usize);
    let mut all_used = used.to_vec();

    for _ in 0..count {
        let name = get_available_name(&all_used);
        all_used.push(name.clone());
        names.push(name);
    }

    names
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
                "pause_mode must be 'sender' or 'all'".to_string()
            ));
        }
    }

    // Parse worker scale
    let scale = WorkerScale::parse(&args.workers)
        .map_err(GoError::InvalidWorkerScale)?;

    // Parse time limit if specified
    let time_limit_minutes = if let Some(ref tl) = args.time_limit {
        Some(parse_time_limit(tl).map_err(GoError::InvalidTimeLimit)?)
    } else {
        None
    };

    // Get run directories
    let run_dir = get_run_dir(&run_name);
    let staging_dir = get_staging_dir(&run_name);
    let db_path = get_db_path(&run_name);

    // Check for existing run
    if run_dir.exists() {
        let state = SQLiteState::new(db_path.clone())?;
        let status = state.status()?;

        match status {
            Status::Working | Status::Eval | Status::Waiting => {
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

    // Get project path (specified, or detect from current directory)
    let project_path = if let Some(ref proj) = args.project {
        let path = Path::new(proj);
        if !path.exists() {
            return Err(GoError::InvalidProject(format!(
                "Project path does not exist: {}",
                proj
            )));
        }
        get_repo_root(Some(path))
            .map_err(|_| GoError::InvalidProject(format!(
                "Not a git repository: {}",
                proj
            )))?
    } else {
        get_repo_root(Some(Path::new(".")))
            .map_err(|_| GoError::NoGitRepo)?
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

    // Create default chats
    let chats_dir = files.chats_dir();
    create_default_user_chat(&chats_dir)?;

    if is_multi_worker {
        create_default_group_chat(&chats_dir, &worker_names, leader.as_deref())?;
    }

    // Create learnings thread
    create_learnings_thread(&chats_dir, &worker_names)?;

    // Clean up staging
    fs::remove_dir_all(&staging_dir)?;

    // Initialize state
    let state = SQLiteState::new(db_path)?;
    state.init_state(Some(project_path.to_str().unwrap_or(".")))?;
    state.set_request(Some(&spec_content))?;
    state.set_worker_scale(&scale.to_string())?;

    // Add scope task if not exists
    let _ = state.add_task("scope", "Read spec, create exploration tasks", None, None);

    // Pre-claim scope for first worker
    let first_worker = &worker_names[0];
    let _ = state.claim_task("scope", first_worker);

    // Create workspace with staging branch
    let runs_dir = get_hirsel_dir().join("runs");
    let workspace_dir = create_workspace(&run_name, &project_path, &runs_dir)?;

    // Create worker clones/worktrees
    let mut worker_dirs: Vec<(String, PathBuf)> = Vec::new();

    for worker_name in &worker_names {
        let worker_dir = if is_multi_worker {
            create_worker_clone(&run_name, &project_path, worker_name, Some(&workspace_dir), &runs_dir)?
        } else {
            workspace_dir.clone()
        };

        worker_dirs.push((worker_name.clone(), worker_dir.clone()));

        // Register worker in state
        state.add_worker(worker_name, worker_dir.to_str().unwrap_or("."), "local")?;

        // Create individual worker chat
        create_worker_chat(&chats_dir, worker_name)?;
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

    // Spawn worker processes (skip for draft runs)
    if !args.draft {
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
                    Some(teammates.iter().filter(|t| *t != worker_name).cloned().collect())
                } else {
                    None
                },
                resume_session_id: None,
            };

            match spawn_worker(config, &state) {
                Ok(result) => {
                    info!(
                        "Spawned worker {} (PID {})",
                        result.worker_name, result.pid
                    );
                }
                Err(WorkerError::RunPaused) => {
                    // Run was paused - don't spawn more workers
                    break;
                }
                Err(e) => {
                    // Log error but continue with other workers
                    eprintln!("Warning: Failed to spawn worker {}: {}", worker_name, e);
                }
            }
        }
    }

    Ok(GoOutput {
        run_name,
        project_path,
        run_dir,
        worker_names,
        worker_count: initial_count,
        time_limit_minutes,
    })
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
    fn test_parse_worker_scale_fixed() {
        let scale = WorkerScale::parse("3").unwrap();
        assert_eq!(scale.min, 3);
        assert_eq!(scale.max, Some(3));
        assert!(!scale.autoscale);
        assert_eq!(scale.initial_count(), 3);
    }

    #[test]
    fn test_parse_worker_scale_range() {
        let scale = WorkerScale::parse("1-5").unwrap();
        assert_eq!(scale.min, 1);
        assert_eq!(scale.max, Some(5));
        assert!(scale.autoscale);
        assert_eq!(scale.initial_count(), 1);
    }

    #[test]
    fn test_parse_worker_scale_unlimited() {
        let scale = WorkerScale::parse("2+").unwrap();
        assert_eq!(scale.min, 2);
        assert_eq!(scale.max, None);
        assert!(scale.autoscale);
        assert_eq!(scale.initial_count(), 2);
    }

    #[test]
    fn test_parse_worker_scale_invalid() {
        assert!(WorkerScale::parse("0").is_err());
        assert!(WorkerScale::parse("5-3").is_err());
        assert!(WorkerScale::parse("abc").is_err());
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
        let used = vec!["achilles".to_string(), "hector".to_string()];
        let name = get_available_name(&used);
        assert_eq!(name, "ajax");
    }

    #[test]
    fn test_get_available_names() {
        let names = get_available_names(3, &[]);
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], "achilles");
        assert_eq!(names[1], "hector");
        assert_eq!(names[2], "ajax");
    }
}
