//! Core modules for hirsel - shared between CLI and GUI
//!
//! These modules implement the business logic for hirsel, including:
//! - State management (SQLite)
//! - Configuration
//! - Git operations
//! - Chat/messaging system
//! - File utilities
//! - ACP client
//! - Eval system

pub mod acp;
pub mod chat_session;
pub mod chats;
pub mod compaction;
pub mod config;
pub mod coordinator_api;
pub mod eval;
pub mod files;
pub mod git;
pub mod git_http;
pub mod metrics;
pub mod names;
pub mod remote;
pub mod state;
pub mod state_access;
pub mod tunnel;
pub mod workers;

// Re-export commonly used types
pub use acp::{ACPClientConfig, ACPError, MCPServerConfig, SessionUpdate};
pub use chat_session::{
    ChatEvent, ChatSessionConfig, ChatSessionError, ChatSessionManager, PendingPermission,
    PermissionOption, PermissionResponse, UIContext,
};
pub use chats::{ChatHeader, ChatMode};
pub use config::*;
pub use eval::{
    run_eval_acp, run_eval_from_args, EvalAcpConfig, EvalAcpResult, EvalConfig, EvalError,
    EvalResult,
};
pub use files::Files;
pub use names::{generate_unique_names, generate_worker_name};
pub use remote::{
    parse_remote_spec, parse_remote_specs, RemoteConfig, RemoteError, RemoteResult,
    RemoteWorkerSpawner,
};
pub use state::*;
pub use state_access::{StateAccess, StateAccessError, StateAccessResult};
pub use workers::{
    check_and_send_time_notifications, check_time_expired, check_worker_heartbeats,
    get_agent_command, handle_time_expired, is_pid_alive, maybe_scale_up, maybe_trigger_eval,
    pause_all_workers, resume_awaiting_workers, spawn_worker, update_worker_heartbeat, SpawnResult,
    WorkerError, WorkerResult, WorkerScale, WorkerSpawnConfig,
};
