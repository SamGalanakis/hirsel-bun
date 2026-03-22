//! Minimal backend/admin command surface plus hidden worker entrypoints.
//!
//! Hirsel no longer exposes the full product through the terminal. The
//! desktop/mobile UI is the primary interface. The command line exists for:
//! - backend self-hosting (`hirsel serve`)
//! - hidden internal worker/admin subprocess entrypoints
//! - the worker-side command parser used inside spawned agent runtimes

pub mod config;
pub mod scribe;

use clap::{Args, Parser, Subcommand};

pub use config::{get_agent_command, AgentPreset};

/// Hirsel backend and worker runtime.
#[derive(Parser, Debug)]
#[command(name = "hirsel")]
#[command(version = crate::version::FULL_VERSION, about = "Hirsel backend and worker runtime", long_about = None)]
pub struct Cli {
    /// Show detailed build information
    #[arg(long)]
    pub build_info: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Backend/admin commands plus hidden internal entrypoints.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run the Hirsel backend server
    #[cfg(feature = "server")]
    Serve(ServeArgs),

    /// Run worker subprocess (internal, called by spawn_worker)
    #[command(name = "__worker-run", hide = true)]
    WorkerRun(InternalWorkerRunArgs),

    /// Run eval MCP server (internal, called by eval agent)
    #[command(name = "__eval-mcp", hide = true)]
    EvalMcp,

    /// Run worker MCP server (internal, called by worker agent)
    #[command(name = "__worker-mcp", hide = true)]
    WorkerMcp,

    /// Run eval agent (internal, spawned by lifecycle manager)
    #[command(name = "__eval-run", hide = true)]
    EvalRun(InternalEvalRunArgs),

    /// Run scribe processing (internal, spawned by daemon)
    #[command(name = "__scribe", hide = true)]
    Scribe(RunNameArg),

    /// Run board MCP server for Shepherd (internal)
    #[command(name = "__board-mcp", hide = true)]
    BoardMcp,

    /// Run as daemon (internal, auto-started by the desktop app)
    #[cfg(feature = "server")]
    #[command(name = "__daemon", hide = true)]
    Daemon(DaemonArgs),
}

/// Arguments for `hirsel serve`
#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    pub port: u16,
}

/// Simple run name argument
#[derive(Args, Debug)]
pub struct RunNameArg {
    /// Name of the run
    pub run_name: String,
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

    /// Assigned task ID (direct task assignment)
    #[arg(long)]
    pub assigned_task_id: Option<String>,

    /// Whether the assigned task is a plan task
    #[arg(long, default_value = "false")]
    pub plan_task: bool,
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

    /// Agent command (JSON array)
    #[arg(long)]
    pub agent_command: String,
}

/// Arguments for `hirsel __daemon` (internal)
#[derive(Args, Debug)]
pub struct DaemonArgs {
    /// Idle timeout in seconds (daemon exits if no active runs for this long)
    #[arg(long, default_value = "300")]
    pub idle_timeout: u64,

    /// TCP port for HTTP server
    #[arg(long, default_value = "19700")]
    pub tcp_port: u16,
}

/// Worker-side parser used inside spawned worker runtimes.
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

    /// Mark current or specified task as done
    Done(WorkerTaskDoneArgs),
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

/// Arguments for worker task done (task_id is optional)
#[derive(Args, Debug)]
pub struct WorkerTaskDoneArgs {
    /// Task ID (uses currently claimed task if not specified)
    pub task_id: Option<String>,
}

/// Arguments for worker msg send
#[derive(Args, Debug)]
pub struct WorkerMsgSendArgs {
    /// Thread name: "user" for DM to human, "group" for team chat
    pub thread: String,

    /// Message content
    pub message: String,
}

/// Arguments for worker msg read
#[derive(Args, Debug)]
pub struct WorkerMsgReadArgs {
    /// Thread name (reads all if not specified)
    pub thread: Option<String>,
}

/// Parse and return the main CLI arguments.
pub fn parse_cli() -> Cli {
    Cli::parse()
}

/// Parse and return worker CLI arguments.
pub fn parse_worker_cli() -> WorkerCli {
    WorkerCli::parse()
}

/// Check if running as worker subprocess.
pub fn is_worker_subprocess() -> bool {
    std::env::var("HIRSEL_WORKER_SUBPROCESS").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_no_args() {
        let cli = Cli::try_parse_from(["hirsel"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.build_info);
    }

    #[test]
    fn test_cli_serve_command() {
        let cli = Cli::try_parse_from(["hirsel", "serve", "--port", "9090"]).unwrap();
        if let Some(Commands::Serve(args)) = cli.command {
            assert_eq!(args.port, 9090);
        } else {
            panic!("Expected Serve command");
        }
    }

    #[test]
    fn test_worker_cli_msg_send() {
        let cli =
            WorkerCli::try_parse_from(["hirsel-worker", "msg", "send", "group", "Hello team!"])
                .unwrap();

        if let WorkerCommands::Msg(MsgSubcommands::Send(args)) = cli.command {
            assert_eq!(args.thread, "group");
            assert_eq!(args.message, "Hello team!");
        } else {
            panic!("Expected Msg Send command");
        }
    }

    #[test]
    fn test_worker_cli_task_done_optional_id() {
        let cli = WorkerCli::try_parse_from(["hirsel-worker", "task", "done", "my_task"]).unwrap();
        if let WorkerCommands::Task(TaskSubcommands::Done(args)) = cli.command {
            assert_eq!(args.task_id, Some("my_task".to_string()));
        } else {
            panic!("Expected Task Done command");
        }

        let cli = WorkerCli::try_parse_from(["hirsel-worker", "task", "done"]).unwrap();
        if let WorkerCommands::Task(TaskSubcommands::Done(args)) = cli.command {
            assert!(args.task_id.is_none());
        } else {
            panic!("Expected Task Done command");
        }
    }
}
