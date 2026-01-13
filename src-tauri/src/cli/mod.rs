//! CLI command routing for hirsel.
//!
//! This module defines the command-line interface using clap, providing
//! two entry points:
//! - `hirsel` - Main CLI with run management, task admin, and configuration
//! - `hirsel-worker` - Worker subprocess commands for AI agents
//!
//! When invoked without arguments, `hirsel` launches the native GUI.

pub mod completions;
pub mod config;
pub mod deliver;
pub mod go;
pub mod man;
pub mod pause;
pub mod resume;
pub mod runs;
pub mod spec;
pub mod tasks;
pub mod templates;
pub mod view;

use clap::{Args, Parser, Subcommand};

// Re-export command implementations
pub use completions::{generate_completions, print_completions, run_completions};
pub use config::{
    agent_presets, get_current_agent, run_config, set_agent, AgentPreset,
};
pub use go::{run as run_go, GoError, GoOutput, GoResult};
pub use man::run_man;
pub use runs::list_runs;
pub use spec::{read_spec, run_spec, update_spec_amendments, Amendment, SpecError};
pub use tasks::{
    run_task_add, run_task_delete, run_task_done, run_task_reopen, run_task_unclaim, run_tasks,
    TaskError,
};
pub use templates::{
    get_template, get_templates_dir, list_templates, read_template_eval, read_template_spec,
    run_templates, Template, TemplateError,
};
pub use pause::run_pause;
pub use resume::{run_resume, parse_time_limit};

/// Hirsel - Herd your AI coding agents
#[derive(Parser, Debug)]
#[command(name = "hirsel")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Output in JSON format (for scripting)
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Main CLI subcommands
#[derive(Subcommand, Debug)]
pub enum Commands {
    // ========== Run Management ==========
    /// Start a new run
    Go(GoArgs),

    /// View run status
    View(RunNameArg),

    /// View activity log
    Log(LogArgs),

    /// Watch worker live (attaches to tmux session)
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
}

// ========== Argument structs ==========

/// Arguments for `hirsel go`
#[derive(Args, Debug)]
pub struct GoArgs {
    /// Name for this run
    pub run_name: String,

    /// Path to spec file or template name
    pub spec: String,

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
        let cli =
            WorkerCli::try_parse_from(["hirsel-worker", "task", "done", "my_task"]).unwrap();
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
