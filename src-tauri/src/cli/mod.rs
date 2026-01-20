//! CLI command routing for hirsel.
//!
//! This module defines the command-line interface using clap, providing
//! two entry points:
//! - `hirsel` - Main CLI with run management, task admin, and configuration
//! - `hirsel-worker` - Worker subprocess commands for AI agents
//!
//! When invoked without arguments, `hirsel` launches the native GUI.

pub mod acp_bridge;
pub mod asset;
#[cfg(feature = "tui")]
pub mod attach;
pub mod compact;
pub mod completions;
pub mod config;
pub mod delete;
pub mod deliver;
pub mod diff;
#[cfg(feature = "full-cli")]
pub mod go;
pub mod improve;
pub mod log;
pub mod man;
pub mod msg;
pub mod pause;
pub mod prune;
pub mod reset;
pub mod resume;
pub mod runs;
pub mod spec;
pub mod summary;
pub mod tasks;
pub mod templates;
#[cfg(feature = "full-cli")]
pub mod test;
#[cfg(feature = "tui")]
pub mod tui;
pub mod view;

use clap::{Args, Parser, Subcommand};

// Re-export command implementations
pub use self::diff::{print_diff, run_diff, DiffError, DiffResult};
pub use asset::run_asset;
#[cfg(feature = "tui")]
pub use attach::{list_targets, run_attach};
pub use completions::{generate_completions, print_completions, run_completions};
pub use config::{
    agent_presets, get_agent_command, get_current_agent, run_config, set_agent, AgentPreset,
};
pub use delete::execute as run_delete;
#[cfg(feature = "full-cli")]
pub use go::{run as run_go, GoError, GoOutput, GoResult};
pub use log::{run_log, LogResult, OutputFormat};
pub use man::run_man;
pub use msg::{get_available_threads, run as run_msg, MsgError, MsgOutput, MsgResult, ThreadInfo};
pub use pause::run_pause;
pub use prune::execute as run_prune;
pub use reset::{run_reset, ResetTarget};
pub use resume::{parse_time_limit, run_resume};
pub use runs::list_runs;
pub use spec::{read_spec, run_spec, update_spec_amendments, Amendment, SpecError};
pub use summary::{get_summary_text, has_summary, run_summary, SummaryError};
pub use templates::{
    get_template, get_templates_dir, list_templates, read_template_eval, read_template_spec,
    run_templates, Template, TemplateError,
};

/// Hirsel - Herd your AI coding agents
#[derive(Parser, Debug)]
#[command(name = "hirsel")]
#[command(version = crate::version::FULL_VERSION, about, long_about = None)]
pub struct Cli {
    /// Output in JSON format (for scripting)
    #[arg(long, global = true)]
    pub json: bool,

    /// Use a specific orchestrator profile (from config)
    #[arg(long, short = 'p', global = true)]
    pub profile: Option<String>,

    /// Show detailed build information
    #[arg(long)]
    pub build_info: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Main CLI subcommands
#[derive(Subcommand, Debug)]
pub enum Commands {
    // ========== Run Management ==========
    /// Start a new run
    #[cfg(feature = "full-cli")]
    Go(GoArgs),

    /// View run status
    View(RunNameArg),

    /// View activity log
    Log(LogArgs),

    /// Watch worker live output (TUI)
    #[cfg(feature = "tui")]
    Attach(AttachArgs),

    /// Send message to run
    Msg(MsgArgs),

    /// Show code changes via git diff
    Diff(RunNameArg),

    /// Create branch in target repo
    Deliver(DeliverArgs),

    /// Pause all workers in a run
    Pause(RunNameArg),

    /// Resume a paused or timed-out run
    Resume(ResumeArgs),

    /// Remove a run
    Delete(RunNameArg),

    /// Clone a run to a new draft
    Clone(CloneArgs),

    /// Remove all delivered runs
    Prune,

    /// List all runs
    Runs,

    /// Generate or view run summary
    Summary(SummaryArgs),

    /// Set run mode (hitl/yolo)
    Mode(ModeArgs),

    /// Add amendment to spec
    Amend(AmendArgs),

    /// View/edit spec
    Spec(RunNameArg),

    /// Add assets (images, files) to a run
    Asset(AssetArgs),

    // ========== Task Management ==========
    /// List tasks in a run
    Tasks(RunNameArg),

    /// Add task to a run
    #[command(name = "task-add")]
    TaskAdd(TaskAddArgs),

    /// Delete a task from a run
    #[command(name = "task-delete")]
    TaskDelete(TaskIdArgs),

    /// Mark task as done (admin)
    #[command(name = "task-done")]
    TaskDone(TaskIdArgs),

    /// Reopen a completed task
    #[command(name = "task-reopen")]
    TaskReopen(TaskIdArgs),

    /// Unclaim a task
    #[command(name = "task-unclaim")]
    TaskUnclaim(TaskIdArgs),

    // ========== Configuration ==========
    /// Configure agent (interactive or set directly)
    Config(ConfigArgs),

    /// List spec templates
    Templates,

    /// Install shell completions
    Completions(CompletionsArgs),

    /// Show manual
    Man(ManArgs),

    /// Update project memory from learnings
    Improve(ImproveArgs),

    /// Reset runs and/or config (requires typing 'reset' to confirm)
    Reset(ResetArgs),

    /// Run e2e test scenarios
    #[cfg(feature = "full-cli")]
    Test(TestArgs),

    /// Run as HTTP server (headless mode for remote orchestration)
    #[cfg(feature = "server")]
    Serve(ServeArgs),

    // ========== Internal ==========
    /// Run worker subprocess (internal, called by spawn_worker)
    #[command(name = "__worker-run", hide = true)]
    WorkerRun(InternalWorkerRunArgs),

    /// Run eval MCP server (internal, called by eval agent)
    #[command(name = "__eval-mcp", hide = true)]
    EvalMcp,

    /// Run worker MCP server (internal, called by worker agent)
    #[command(name = "__worker-mcp", hide = true)]
    WorkerMcp,

    /// Run eval agent (internal, spawned by maybe_trigger_eval)
    #[command(name = "__eval-run", hide = true)]
    EvalRun(InternalEvalRunArgs),

    /// Run learnings compaction (internal, spawned by GUI polling)
    #[command(name = "__compact-learnings", hide = true)]
    CompactLearnings(RunNameArg),

    /// Run remote worker (internal, spawned on remote machine via SSH)
    #[command(name = "__remote-worker", hide = true)]
    RemoteWorker(RemoteWorkerArgs),

    /// Run ACP bridge server for Claude CLI (internal, used as agent command)
    #[command(name = "__acp-bridge", hide = true)]
    AcpBridge,

    /// Run as daemon (internal, auto-started by CLI)
    #[cfg(feature = "server")]
    #[command(name = "__daemon", hide = true)]
    Daemon(DaemonArgs),

    /// Stop the daemon
    #[cfg(feature = "server")]
    #[command(name = "daemon")]
    DaemonCtl(DaemonCtlArgs),

    // ========== Completion Helpers ==========
    /// List run names (for shell completion)
    #[command(name = "_complete_runs", hide = true)]
    CompleteRuns,

    /// List worker names for a run (for shell completion)
    #[command(name = "_complete_workers", hide = true)]
    CompleteWorkers(RunNameArg),

    /// List thread names for a run (for shell completion)
    #[command(name = "_complete_threads", hide = true)]
    CompleteThreads(RunNameArg),
}

/// Arguments for internal worker run command
#[derive(Args, Debug)]
pub struct InternalWorkerRunArgs {
    /// Run name
    #[arg(long)]
    pub run: String,

    /// Worker name
    #[arg(long)]
    pub worker: String,

    /// Work directory
    #[arg(long)]
    pub work_dir: String,

    /// Run directory
    #[arg(long)]
    pub run_dir: String,

    /// Spec file path
    #[arg(long)]
    pub spec: String,

    /// Agent command (JSON array)
    #[arg(long)]
    pub agent_command: String,

    /// Is leader
    #[arg(long, default_value = "false")]
    pub is_leader: bool,

    /// Leader name
    #[arg(long)]
    pub leader_name: Option<String>,

    /// Teammates (comma-separated)
    #[arg(long)]
    pub teammates: Option<String>,

    /// Resume session ID
    #[arg(long)]
    pub resume_session_id: Option<String>,
}

/// Arguments for internal eval run command
#[derive(Args, Debug)]
pub struct InternalEvalRunArgs {
    /// Run name
    #[arg(long)]
    pub run: String,

    /// Run directory
    #[arg(long)]
    pub run_dir: String,

    /// Spec file path
    #[arg(long)]
    pub spec: String,

    /// Eval spec file path
    #[arg(long)]
    pub eval_spec: String,

    /// Agent command (JSON array)
    #[arg(long)]
    pub agent_command: String,
}

/// Arguments for remote worker command
#[derive(Args, Debug)]
pub struct RemoteWorkerArgs {
    /// Coordinator API URL (via SSH tunnel)
    #[arg(long)]
    pub api_url: String,

    /// Run name
    #[arg(long)]
    pub run_name: String,

    /// Worker name
    #[arg(long)]
    pub worker_name: String,

    /// Work directory on remote machine
    #[arg(long)]
    pub work_dir: String,

    /// Agent command (JSON array)
    #[arg(long)]
    pub agent_command: String,

    /// Spec file path
    #[arg(long)]
    pub spec: String,

    /// Is leader
    #[arg(long, default_value = "false")]
    pub is_leader: bool,

    /// Leader name
    #[arg(long)]
    pub leader_name: Option<String>,

    /// Teammates (comma-separated)
    #[arg(long)]
    pub teammates: Option<String>,

    /// Wait for file upload via HTTP before starting worker
    #[arg(long, default_value = "false")]
    pub wait_for_files: bool,

    /// Port for file receiver (default: 19800)
    #[arg(long)]
    pub file_receiver_port: Option<u16>,
}

// ========== Argument structs ==========

/// Arguments for `hirsel go`
#[derive(Args, Debug)]
pub struct GoArgs {
    /// Name for this run
    pub run_name: String,

    /// Path to spec file or template name
    pub spec: String,

    /// Orchestrator profile to use (for remote server mode)
    #[arg(long)]
    pub profile: Option<String>,

    /// Number or range of workers (e.g., "3", "1-5", "2+")
    #[arg(short, long, default_value = "1")]
    pub workers: String,

    /// Time limit (e.g., "30m", "1h", "1h30m")
    #[arg(short, long)]
    pub time_limit: Option<String>,

    /// Remote worker spec (e.g., "user@host:2")
    #[arg(long)]
    pub remote: Option<String>,

    /// Run in sandbox mode
    #[arg(long)]
    pub sandbox: bool,

    /// YOLO mode (skip confirmation prompts)
    #[arg(long)]
    pub yolo: bool,

    /// Path to eval script
    #[arg(long)]
    pub eval: Option<String>,

    /// Use a template instead of spec file
    #[arg(long)]
    pub template: Option<String>,

    /// Project path (defaults to current directory)
    #[arg(short = 'P', long)]
    pub project: Option<String>,

    /// Maximum iterations before auto-pause
    #[arg(long)]
    pub max_iterations: Option<i64>,

    /// Pause behavior when messaging user: "sender" or "all"
    #[arg(long)]
    pub pause_mode: Option<String>,

    /// Create a draft run (don't spawn workers until explicitly started)
    #[arg(long)]
    pub draft: bool,

    /// Path to assets folder (copies contents to run's assets/)
    #[arg(long)]
    pub assets: Option<String>,

    /// Runner to use for workers (e.g., "local", "sprites", or a named runner from config)
    #[arg(long)]
    pub runner: Option<String>,
}

/// Simple run name argument
#[derive(Args, Debug)]
pub struct RunNameArg {
    /// Name of the run
    pub run_name: String,
}

/// Arguments for `hirsel log`
#[derive(Args, Debug)]
pub struct LogArgs {
    /// Name of the run
    pub run_name: String,

    /// Follow log output (like tail -f)
    #[arg(short, long)]
    pub follow: bool,

    /// Number of lines to show
    #[arg(short, long, default_value = "50")]
    pub limit: usize,
}

/// Arguments for `hirsel attach`
#[derive(Args, Debug)]
pub struct AttachArgs {
    /// Name of the run
    pub run_name: String,

    /// Worker name or eval name to attach to
    pub target: Option<String>,
}

/// Arguments for `hirsel msg`
#[derive(Args, Debug)]
pub struct MsgArgs {
    /// Name of the run
    pub run_name: String,

    /// Message to send (if omitted, shows messages)
    pub message: Option<String>,

    /// Thread to send to (default: user)
    #[arg(short, long, default_value = "user")]
    pub thread: String,

    /// List available threads
    #[arg(long)]
    pub list_threads: bool,
}

/// Arguments for `hirsel deliver`
#[derive(Args, Debug)]
pub struct DeliverArgs {
    /// Name of the run
    pub run_name: String,

    /// Branch name to create (default: hirsel/<run_name>)
    #[arg(short, long)]
    pub branch: Option<String>,
}

/// Arguments for `hirsel resume`
#[derive(Args, Debug)]
pub struct ResumeArgs {
    /// Name of the run
    pub run_name: String,

    /// New time limit (e.g., "30m", "1h")
    #[arg(long)]
    pub time_limit: Option<String>,
}

/// Arguments for `hirsel clone`
#[derive(Args, Debug)]
pub struct CloneArgs {
    /// Name of the run to clone
    pub source_run: String,

    /// Name for the new run
    pub new_name: String,
}

/// Arguments for `hirsel summary`
#[derive(Args, Debug)]
pub struct SummaryArgs {
    /// Name of the run
    pub run_name: String,

    /// Regenerate summary even if one exists
    #[arg(long)]
    pub regenerate: bool,
}

/// Arguments for `hirsel mode`
#[derive(Args, Debug)]
pub struct ModeArgs {
    /// Name of the run
    pub run_name: String,

    /// New mode (hitl or yolo)
    pub new_mode: String,
}

/// Arguments for `hirsel amend`
#[derive(Args, Debug)]
pub struct AmendArgs {
    /// Name of the run
    pub run_name: String,

    /// Amendment message
    pub message: String,
}

/// Arguments for `hirsel asset`
#[derive(Args, Debug)]
pub struct AssetArgs {
    /// Name of the run
    pub run_name: String,

    /// Paths to files to add as assets
    #[arg(required = true)]
    pub paths: Vec<String>,
}

/// Arguments for task commands that need run + task_id
#[derive(Args, Debug)]
pub struct TaskIdArgs {
    /// Name of the run
    pub run_name: String,

    /// Task ID
    pub task_id: String,
}

/// Arguments for `hirsel task-add`
#[derive(Args, Debug)]
pub struct TaskAddArgs {
    /// Name of the run
    pub run_name: String,

    /// Task ID (lowercase, letters and underscores)
    pub task_id: String,

    /// Task description
    pub description: String,

    /// Parent task ID
    #[arg(long)]
    pub parent: Option<String>,

    /// Tasks that must complete before this one
    #[arg(long)]
    pub blocked_by: Vec<String>,
}

/// Arguments for `hirsel config`
#[derive(Args, Debug)]
pub struct ConfigArgs {
    /// Agent preset to set (claude, gemini, opencode, codex, goose)
    pub agent: Option<String>,
}

/// Arguments for `hirsel completions`
#[derive(Args, Debug)]
pub struct CompletionsArgs {
    /// Shell to generate completions for (bash, zsh, fish, elvish, powershell)
    /// If provided, prints completion script to stdout
    pub shell: Option<String>,

    /// Force reinstall completions
    #[arg(long)]
    pub force: bool,
}

/// Arguments for `hirsel man`
#[derive(Args, Debug)]
pub struct ManArgs {
    /// Show agent-specific manual
    #[arg(long)]
    pub agent: bool,
}

/// Arguments for `hirsel improve`
#[derive(Args, Debug)]
pub struct ImproveArgs {
    /// Run name (optional, uses current directory context if not provided)
    pub run_name: Option<String>,
}

/// Arguments for `hirsel reset`
#[derive(Args, Debug)]
pub struct ResetArgs {
    /// Only delete runs, keep config
    #[arg(long, conflicts_with_all = ["config", "all"])]
    pub runs: bool,

    /// Only reset config to defaults, keep runs
    #[arg(long, conflicts_with_all = ["runs", "all"])]
    pub config: bool,

    /// Reset everything (runs and config)
    #[arg(long, conflicts_with_all = ["runs", "config"])]
    pub all: bool,

    /// Confirmation string (must be "reset" to proceed)
    #[arg(long, short = 'y')]
    pub confirm: Option<String>,
}

/// Arguments for `hirsel test`
#[derive(Args, Debug)]
pub struct TestArgs {
    /// Scenario name to run (lists available scenarios if omitted)
    pub scenario: Option<String>,

    /// Custom run name (default: test-<scenario>)
    #[arg(short = 'n', long)]
    pub run_name: Option<String>,

    /// Number of workers
    #[arg(short, long, default_value = "1")]
    pub workers: String,

    /// YOLO mode (skip confirmation prompts)
    #[arg(long)]
    pub yolo: bool,

    /// Remote worker spec (e.g., "user@host:2")
    #[arg(long)]
    pub remote: Option<String>,

    /// Runner to use for workers (e.g., "local", "sprites", or a named runner from config)
    #[arg(long)]
    pub runner: Option<String>,
}

/// Arguments for `hirsel serve`
#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    pub port: u16,
}

/// Arguments for `hirsel __daemon` (internal)
#[derive(Args, Debug)]
pub struct DaemonArgs {
    /// Idle timeout in seconds (daemon exits if no active runs for this long)
    #[arg(long, default_value = "300")]
    pub idle_timeout: u64,
}

/// Arguments for `hirsel daemon`
#[derive(Args, Debug)]
pub struct DaemonCtlArgs {
    /// Daemon subcommand
    #[command(subcommand)]
    pub command: DaemonCommand,
}

/// Daemon control subcommands
#[derive(Subcommand, Debug)]
pub enum DaemonCommand {
    /// Start the daemon (if not running)
    Start,
    /// Stop the daemon
    Stop,
    /// Check daemon status
    Status,
}

// ========== Worker CLI (hirsel-worker) ==========

/// Hirsel Worker - Commands for AI agents inside runs
#[derive(Parser, Debug)]
#[command(name = "hirsel-worker")]
#[command(version, about = "Worker commands for AI agents")]
pub struct WorkerCli {
    #[command(subcommand)]
    pub command: WorkerCommands,
}

/// Worker CLI subcommands
#[derive(Subcommand, Debug)]
pub enum WorkerCommands {
    /// Signal that all work is complete (triggers eval if configured)
    Done,

    /// Task management commands
    #[command(subcommand)]
    Task(TaskSubcommands),

    /// Messaging commands
    #[command(subcommand)]
    Msg(MsgSubcommands),
}

/// Worker task subcommands
#[derive(Subcommand, Debug)]
pub enum TaskSubcommands {
    /// List all tasks
    List,

    /// Add a new task
    Add(WorkerTaskAddArgs),

    /// Claim a task to work on
    Claim(WorkerTaskIdArg),

    /// Mark current or specified task as done
    Done(WorkerTaskDoneArgs),

    /// Release a claimed task without completing it
    Unclaim(WorkerTaskDoneArgs),

    /// Reopen a completed task
    Undone(WorkerTaskIdArg),

    /// Delete a task
    Delete(WorkerTaskIdArg),

    /// Wait for tasks to become available
    Await,
}

/// Worker message subcommands
#[derive(Subcommand, Debug)]
pub enum MsgSubcommands {
    /// Send a message to a thread
    Send(WorkerMsgSendArgs),

    /// Read messages from a thread
    Read(WorkerMsgReadArgs),

    /// List available threads
    List,

    /// Check inbox for new messages
    Inbox,
}

/// Arguments for worker task add
#[derive(Args, Debug)]
pub struct WorkerTaskAddArgs {
    /// Task ID
    pub task_id: String,

    /// Task name/description
    pub name: String,

    /// Parent task ID
    #[arg(long)]
    pub parent: Option<String>,

    /// Tasks that must complete before this one
    #[arg(long)]
    pub blocked_by: Vec<String>,
}

/// Simple task ID argument for workers
#[derive(Args, Debug)]
pub struct WorkerTaskIdArg {
    /// Task ID
    pub task_id: String,
}

/// Arguments for worker task done/unclaim (task_id is optional)
#[derive(Args, Debug)]
pub struct WorkerTaskDoneArgs {
    /// Task ID (uses currently claimed task if not specified)
    pub task_id: Option<String>,
}

/// Arguments for worker msg send
#[derive(Args, Debug)]
pub struct WorkerMsgSendArgs {
    /// Thread name (e.g., "user", "group")
    pub thread: String,

    /// Message content
    pub message: String,

    /// Wait for reply before continuing
    #[arg(long)]
    pub wait: bool,
}

/// Arguments for worker msg read
#[derive(Args, Debug)]
pub struct WorkerMsgReadArgs {
    /// Thread name (reads all if not specified)
    pub thread: Option<String>,
}

// ========== CLI Execution ==========

/// Parse and return the main CLI arguments
pub fn parse_cli() -> Cli {
    Cli::parse()
}

/// Parse and return worker CLI arguments
pub fn parse_worker_cli() -> WorkerCli {
    WorkerCli::parse()
}

/// Check if running as worker subprocess
pub fn is_worker_subprocess() -> bool {
    std::env::var("HIRSEL_WORKER_SUBPROCESS").is_ok()
}

/// Run the CLI with the given parsed arguments
/// Returns Ok(true) if a command was executed, Ok(false) if no command (launch GUI)
pub fn run_cli() -> anyhow::Result<bool> {
    let cli = parse_cli();

    let Some(command) = cli.command else {
        // No command - return false to indicate GUI should launch
        return Ok(false);
    };

    let json = cli.json;

    match command {
        // Run Management
        #[cfg(feature = "full-cli")]
        Commands::Go(args) => {
            match go::run(&args) {
                Ok(output) => {
                    if json {
                        // Build JSON manually since GoOutput doesn't impl Serialize
                        let json_output = serde_json::json!({
                            "run_name": output.run_name,
                            "project_path": output.project_path,
                            "run_dir": output.run_dir,
                            "worker_names": output.worker_names,
                            "worker_count": output.worker_count,
                            "time_limit_minutes": output.time_limit_minutes,
                        });
                        println!("{}", serde_json::to_string_pretty(&json_output)?);
                    } else {
                        println!(
                            "Started run '{}' with {} workers",
                            output.run_name, output.worker_count
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::View(args) => {
            if let Err(e) = view::execute(&args.run_name, json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Log(args) => {
            let format = if json {
                log::OutputFormat::Json
            } else {
                log::OutputFormat::Pretty
            };
            match log::run_log(&args.run_name, args.follow, args.limit, format) {
                log::LogResult::Success | log::LogResult::Empty | log::LogResult::Interrupted => {}
                log::LogResult::Error(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        #[cfg(feature = "tui")]
        Commands::Attach(args) => {
            if let Err(e) = attach::run_attach(&args.run_name, args.target.as_deref(), json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Msg(args) => {
            match msg::run(&args) {
                Ok(output) => {
                    if json {
                        // Build JSON manually since MsgOutput doesn't impl Serialize
                        let json_output = match &output {
                            msg::MsgOutput::Sent {
                                thread,
                                resumed_workers,
                            } => serde_json::json!({
                                "type": "sent",
                                "thread": thread,
                                "resumed_workers": resumed_workers,
                            }),
                            msg::MsgOutput::Messages {
                                run_name,
                                thread,
                                messages,
                            } => serde_json::json!({
                                "type": "messages",
                                "run_name": run_name,
                                "thread": thread,
                                "messages": messages.iter().map(|m| serde_json::json!({
                                    "timestamp": m.timestamp,
                                    "sender": m.sender,
                                    "content": m.content,
                                })).collect::<Vec<_>>(),
                            }),
                            msg::MsgOutput::ThreadList { run_name, threads } => serde_json::json!({
                                "type": "thread_list",
                                "run_name": run_name,
                                "threads": threads.iter().map(|t| serde_json::json!({
                                    "name": t.name,
                                    "message_count": t.message_count,
                                })).collect::<Vec<_>>(),
                            }),
                        };
                        println!("{}", serde_json::to_string_pretty(&json_output)?);
                    } else {
                        match output {
                            msg::MsgOutput::Sent { .. } => println!("Message sent"),
                            msg::MsgOutput::Messages { messages, .. } => {
                                for m in messages {
                                    println!("[{}] {}: {}", m.timestamp, m.sender, m.content);
                                }
                            }
                            msg::MsgOutput::ThreadList { threads, .. } => {
                                println!("Available threads:");
                                for t in threads {
                                    println!("  {} ({} messages)", t.name, t.message_count);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Diff(args) => match diff::run_diff(&args.run_name, false) {
            Ok(result) => {
                diff::print_diff(&result, json);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        Commands::Deliver(args) => {
            if let Err(e) = deliver::execute(&args.run_name, args.branch.as_deref(), json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Pause(args) => {
            if let Err(e) = pause::run_pause(&args.run_name, json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Resume(args) => {
            if let Err(e) = resume::run_resume(&args.run_name, args.time_limit.as_deref(), json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Delete(args) => {
            if let Err(e) = delete::execute(&args.run_name, json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Clone(args) => {
            use crate::core::ops::{clone_run, CloneRunConfig};

            let config = CloneRunConfig::new(&args.source_run, &args.new_name);

            match clone_run(config) {
                Ok(result) => {
                    if json {
                        println!(
                            r#"{{"source": "{}", "new_name": "{}", "status": "draft"}}"#,
                            result.source_run, result.new_name
                        );
                    } else {
                        println!(
                            "Cloned '{}' to '{}' (draft)",
                            result.source_run, result.new_name
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Prune => {
            if let Err(e) = prune::execute(json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Runs => {
            if let Err(e) = runs::list_runs(json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Summary(args) => {
            match summary::run_summary(&args.run_name, args.regenerate, json) {
                Ok(output) => {
                    if !json {
                        println!("{}", output);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Mode(args) => {
            use crate::core::{config as core_config, state::SQLiteState, Files};
            let run_dir = core_config::run_dir(&args.run_name);
            let files = Files::new(&run_dir);
            match SQLiteState::new(files.db_path()) {
                Ok(state) => {
                    let hitl = args.new_mode.to_lowercase() == "hitl";
                    if let Err(e) = state.set_human_in_the_loop(hitl) {
                        eprintln!("Error setting mode: {}", e);
                        std::process::exit(1);
                    }
                    if json {
                        println!(r#"{{"mode": "{}"}}"#, if hitl { "hitl" } else { "yolo" });
                    } else {
                        println!("Mode set to {}", if hitl { "hitl" } else { "yolo" });
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Amend(args) => {
            use crate::core::config as core_config;
            let run_dir = core_config::run_dir(&args.run_name);
            // Create an amendment from the message
            let amendment = spec::Amendment {
                id: 0, // Will be assigned by the storage
                message: args.message.clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            match spec::update_spec_amendments(&run_dir, &[amendment]) {
                Ok(_) => {
                    if json {
                        println!(r#"{{"status": "amended"}}"#);
                    } else {
                        println!("Amendment added to spec");
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Spec(args) => {
            use crate::core::config as core_config;
            let run_dir = core_config::run_dir(&args.run_name);
            match spec::run_spec(&run_dir) {
                Ok(content) => {
                    if json {
                        println!("{}", serde_json::to_string(&content)?);
                    } else {
                        println!("{}", content);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Asset(args) => match asset::run_asset(&args.run_name, &args.paths) {
            Ok(added) => {
                if json {
                    println!("{}", serde_json::json!({ "added": added }));
                } else {
                    for file in &added {
                        println!("Added: assets/{}", file);
                    }
                    if let Some(first) = added.first() {
                        println!("\nReference in spec.md: ![description](assets/{})", first);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },

        // Task Management
        Commands::Tasks(args) => match tasks::run_tasks(&args.run_name, json) {
            Ok(output) => println!("{}", output),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        Commands::TaskAdd(args) => {
            match tasks::run_task_add(
                &args.run_name,
                &args.task_id,
                &args.description,
                args.parent.as_deref(),
                &args.blocked_by,
                json,
            ) {
                Ok(output) => println!("{}", output),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::TaskDelete(args) => {
            match tasks::run_task_delete(&args.run_name, &args.task_id, json) {
                Ok(output) => println!("{}", output),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::TaskDone(args) => {
            match tasks::run_task_done(&args.run_name, &args.task_id, json) {
                Ok(output) => println!("{}", output),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::TaskReopen(args) => {
            match tasks::run_task_reopen(&args.run_name, &args.task_id, json) {
                Ok(output) => println!("{}", output),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::TaskUnclaim(args) => {
            match tasks::run_task_unclaim(&args.run_name, &args.task_id, json) {
                Ok(output) => println!("{}", output),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        // Configuration
        Commands::Config(args) => {
            if let Err(e) = config::run_config(args.agent) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Templates => match templates::run_templates(json) {
            Ok(output) => println!("{}", output),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        Commands::Completions(args) => {
            if let Err(e) = completions::run_completions(&args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Man(args) => {
            if let Err(e) = man::run_man(&args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Improve(args) => {
            if let Err(e) = improve::execute(args.run_name.as_deref(), json) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Reset(args) => {
            // Determine target
            let target = if args.all {
                reset::ResetTarget::All
            } else if args.config {
                reset::ResetTarget::Config
            } else if args.runs {
                reset::ResetTarget::Runs
            } else {
                // Default to showing help if no target specified
                eprintln!("Please specify what to reset: --runs, --config, or --all");
                eprintln!();
                eprintln!("Examples:");
                eprintln!("  hirsel reset --runs     Delete all runs");
                eprintln!("  hirsel reset --config   Reset config to defaults");
                eprintln!("  hirsel reset --all      Delete everything");
                std::process::exit(1);
            };

            // Check for confirmation flag
            if let Some(confirm) = &args.confirm {
                if confirm == "reset" {
                    if let Err(e) = reset::execute_reset_confirmed(target, json) {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                } else {
                    eprintln!("Invalid confirmation. Use --confirm reset");
                    std::process::exit(1);
                }
            } else {
                // Interactive mode
                if let Err(e) = reset::run_reset(target, json) {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        #[cfg(feature = "full-cli")]
        Commands::Test(args) => {
            if let Err(e) = test::execute(
                args.scenario.as_deref(),
                args.run_name.as_deref(),
                Some(&args.workers),
                args.yolo,
                json,
                args.remote.as_deref(),
                args.runner.as_deref(),
            ) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::WorkerRun(_args) => {
            // This is handled by lib.rs run_cli() for compatibility
            // Should not reach here in normal CLI flow
            eprintln!("Worker run command should be called via hirsel binary directly");
            std::process::exit(1);
        }
        Commands::EvalMcp => {
            // This is handled by lib.rs run_cli() for compatibility
            // Should not reach here in normal CLI flow
            eprintln!("Eval MCP command should be called via hirsel binary directly");
            std::process::exit(1);
        }
        Commands::WorkerMcp => {
            // This is handled by lib.rs run_cli() for compatibility
            // Should not reach here in normal CLI flow
            eprintln!("Worker MCP command should be called via hirsel binary directly");
            std::process::exit(1);
        }
        Commands::EvalRun(_args) => {
            // This is handled by lib.rs run_cli() for compatibility
            // Should not reach here in normal CLI flow
            eprintln!("Eval run command should be called via hirsel binary directly");
            std::process::exit(1);
        }
        Commands::CompactLearnings(_args) => {
            // This is handled by lib.rs run_cli() for compatibility
            // Should not reach here in normal CLI flow
            eprintln!("Compact learnings command should be called via hirsel binary directly");
            std::process::exit(1);
        }
        Commands::RemoteWorker(_args) => {
            // This is handled by lib.rs run_cli() for compatibility
            // Should not reach here in normal CLI flow
            eprintln!("Remote worker command should be called via hirsel binary directly");
            std::process::exit(1);
        }
        Commands::AcpBridge => {
            // This is handled by lib.rs run_cli() for compatibility
            // Should not reach here in normal CLI flow
            eprintln!("ACP bridge command should be called via hirsel binary directly");
            std::process::exit(1);
        }

        // Completion helpers
        Commands::CompleteRuns => {
            let runs_dir = crate::core::config::runs_dir();
            if runs_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&runs_dir) {
                    let mut runs: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                        .filter(|e| e.path().join("hirsel.db").exists())
                        .filter_map(|e| e.file_name().to_str().map(String::from))
                        .collect();
                    runs.sort();
                    for run in runs {
                        println!("{}", run);
                    }
                }
            }
        }
        Commands::CompleteWorkers(args) => {
            use crate::core::{config as core_config, state::SQLiteState, Files};
            let run_dir = core_config::run_dir(&args.run_name);
            let files = Files::new(&run_dir);
            if files.db_path().exists() {
                if let Ok(state) = SQLiteState::new(files.db_path()) {
                    if let Ok(workers) = state.get_workers() {
                        for worker in workers {
                            println!("{}", worker.name);
                        }
                    }
                }
            }
        }
        Commands::CompleteThreads(args) => {
            use crate::core::{config as core_config, Files};
            let run_dir = core_config::run_dir(&args.run_name);
            let files = Files::new(&run_dir);
            let chats_dir = files.chats_dir();
            if chats_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&chats_dir) {
                    let mut threads: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .filter_map(|e| {
                            let path = e.path();
                            if path.extension().is_some_and(|ext| ext == "md") {
                                path.file_stem().and_then(|s| s.to_str()).map(String::from)
                            } else {
                                None
                            }
                        })
                        .collect();
                    threads.sort();
                    for thread in threads {
                        println!("{}", thread);
                    }
                }
            }
        }
        #[cfg(feature = "server")]
        Commands::Serve(args) => {
            // Server mode - run HTTP server for remote orchestration
            // This is handled in lib.rs run_command, but add here for completeness
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| anyhow::anyhow!("Failed to create runtime: {}", e))?;
            rt.block_on(async { crate::core::server::start_server(args.port).await })
                .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
        }
        #[cfg(feature = "server")]
        Commands::Daemon(_args) => {
            // Daemon command is handled by lib.rs run_command
            eprintln!("Daemon command should be called via hirsel binary directly");
            std::process::exit(1);
        }
        #[cfg(feature = "server")]
        Commands::DaemonCtl(_args) => {
            // DaemonCtl command is handled by lib.rs run_command
            eprintln!("Daemon control command should be called via hirsel binary directly");
            std::process::exit(1);
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_no_args() {
        // Should parse successfully with no command (launches GUI)
        let cli = Cli::try_parse_from(["hirsel"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.json);
    }

    #[test]
    fn test_cli_json_flag() {
        let cli = Cli::try_parse_from(["hirsel", "--json", "runs"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::Runs)));
    }

    #[test]
    fn test_go_command() {
        let cli = Cli::try_parse_from([
            "hirsel",
            "go",
            "my-run",
            "spec.md",
            "--workers",
            "3",
            "--time-limit",
            "30m",
        ])
        .unwrap();

        if let Some(Commands::Go(args)) = cli.command {
            assert_eq!(args.run_name, "my-run");
            assert_eq!(args.spec, "spec.md");
            assert_eq!(args.workers, "3");
            assert_eq!(args.time_limit, Some("30m".to_string()));
        } else {
            panic!("Expected Go command");
        }
    }

    #[test]
    fn test_task_add_command() {
        let cli = Cli::try_parse_from([
            "hirsel",
            "task-add",
            "my-run",
            "implement_auth",
            "Implement authentication",
            "--parent",
            "scope",
        ])
        .unwrap();

        if let Some(Commands::TaskAdd(args)) = cli.command {
            assert_eq!(args.run_name, "my-run");
            assert_eq!(args.task_id, "implement_auth");
            assert_eq!(args.description, "Implement authentication");
            assert_eq!(args.parent, Some("scope".to_string()));
        } else {
            panic!("Expected TaskAdd command");
        }
    }

    #[test]
    fn test_worker_cli_task_claim() {
        let cli = WorkerCli::try_parse_from(["hirsel-worker", "task", "claim", "scope"]).unwrap();

        if let WorkerCommands::Task(TaskSubcommands::Claim(args)) = cli.command {
            assert_eq!(args.task_id, "scope");
        } else {
            panic!("Expected Task Claim command");
        }
    }

    #[test]
    fn test_worker_cli_msg_send() {
        let cli = WorkerCli::try_parse_from([
            "hirsel-worker",
            "msg",
            "send",
            "group",
            "Hello team!",
            "--wait",
        ])
        .unwrap();

        if let WorkerCommands::Msg(MsgSubcommands::Send(args)) = cli.command {
            assert_eq!(args.thread, "group");
            assert_eq!(args.message, "Hello team!");
            assert!(args.wait);
        } else {
            panic!("Expected Msg Send command");
        }
    }

    #[test]
    fn test_worker_cli_task_done_optional_id() {
        // With task_id
        let cli = WorkerCli::try_parse_from(["hirsel-worker", "task", "done", "my_task"]).unwrap();
        if let WorkerCommands::Task(TaskSubcommands::Done(args)) = cli.command {
            assert_eq!(args.task_id, Some("my_task".to_string()));
        } else {
            panic!("Expected Task Done command");
        }

        // Without task_id
        let cli = WorkerCli::try_parse_from(["hirsel-worker", "task", "done"]).unwrap();
        if let WorkerCommands::Task(TaskSubcommands::Done(args)) = cli.command {
            assert!(args.task_id.is_none());
        } else {
            panic!("Expected Task Done command");
        }
    }
}
