//! Core modules for hirsel - shared between CLI and GUI
//!
//! These modules implement the business logic for hirsel, including:
//! - State management (SQLite)
//! - Configuration
//! - Git operations
//! - File utilities
//! - Eval system
//! - Orchestrator abstraction for local/remote coordination

pub mod api_types;
pub mod board;
pub mod capabilities;
pub mod config;
// Intentionally not `pub` — only used by webui.rs
pub mod conflict_resolver;
pub mod constants;
pub mod credentials;
pub mod db;
pub mod delivery;
pub mod delta;
pub mod dispatch;
pub mod draft;
pub mod error;
pub mod eval;
pub mod files;
pub mod forge;
pub mod git;
pub mod github;
pub mod http_client;
pub(crate) mod icons;
pub mod lifecycle;
pub mod llm_provider;
pub mod mcp;
pub mod metrics;
pub mod names;
pub mod ops;
pub mod orchestrator;
pub mod process;
pub mod project;
pub mod route;
pub mod route_runtime;
pub mod run_manager;
pub mod runner;
pub mod scribe;
#[cfg(feature = "server")]
pub mod server;
pub mod shepherd;
pub mod shepherd_chat;
pub mod snapshot;
pub mod state;
pub mod state_access;
pub mod storage;
pub mod system;
pub mod webui;
pub mod worker_concerns;
pub mod workers;
pub mod worktree;

// Re-export commonly used types
pub use config::*;
pub use credentials::{CredentialError, CredentialResult, CredentialStore, ForwardedCredentials};
pub use error::{ErrorKind, HirselError, HirselResult};
pub use eval::{run_eval, run_eval_from_args, EvalAcpConfig, EvalAcpResult, EvalError};
pub use files::Files;
pub use lifecycle::{
    LifecycleAction, LifecycleContext, LifecycleError, LifecycleEvent, LifecycleManager,
    LifecycleResult, LocalLifecycleManager, RunStateMachine, WorkerStateMachine,
};
pub use names::{
    generate_runtime_name, generate_worker_name, get_available_name, get_available_names, slugify,
};
pub use orchestrator::{
    create_local_orchestrator, create_orchestrator, LocalOrchestrator, Orchestrator,
    OrchestratorError, OrchestratorResult, RemoteOrchestrator,
};
pub use project::{
    CreateProjectRequest, Project, ProjectError, ProjectResult, ProjectStore, UpdateProjectRequest,
};
pub use run_manager::{
    create_run_manager, LocalRunManager, RemoteRunManager, RunManager, RunManagerError,
    RunManagerResult,
};
pub use runner::{
    create_runner, LocalRunner, Runner, RunnerConfig, RunnerError, RunnerResult,
    SpawnResult as RunnerSpawnResult, WorkerHandle, WorkerSpawnConfig as RunnerSpawnConfig,
};
pub use scribe::{process_scribe_batch, should_process_batch, ScribeBatchResult, ScribeError};
pub use shepherd::{
    LocalShepherdEngine, ShepherdCommand, ShepherdCommandStatus, ShepherdCommandType,
    ShepherdDecision, ShepherdDecisionType, ShepherdEngine,
};
pub use shepherd_chat::{
    ShepherdChatError, ShepherdChatMessage, ShepherdChatResult, ShepherdChatStore, ShepherdEffort,
    ShepherdQueuedTurn,
};

// Conflict resolver
pub use conflict_resolver::{
    ConflictResolution, ConflictResolutionStatus, ConflictResolverError, ConflictResolverResult,
    ConflictResolverService, ConflictResolverState, ResolutionResult,
};
#[cfg(feature = "s3-storage")]
pub use snapshot::S3ArchiveStrategy;
pub use snapshot::{
    create_archive_strategy, AgentSnapshot, ArchiveHandle, ArchiveResult, ArchiveStrategy,
    NoOpArchiveStrategy, SnapshotError, SnapshotResult, WorkDirSnapshot, WorkerStateHandle,
};
pub use state::*;
pub use state_access::{StateAccess, StateAccessError, StateAccessResult};
#[cfg(feature = "s3-storage")]
pub use storage::S3FileStorage;
pub use storage::{
    create_file_storage, FileStorage, LocalFileStorage, StorageError, StorageResult,
};
pub use worker_concerns::{
    CreateWorkerConcernRequest, WorkerConcern, WorkerConcernError, WorkerConcernResult,
    WorkerConcernStore,
};
pub use workers::{
    check_and_send_time_notifications, check_worker_heartbeats, get_agent_command, is_pid_alive,
    spawn_worker, update_worker_heartbeat, SpawnResult, WorkerError, WorkerResult,
    WorkerSpawnConfig,
};
pub use worktree::{
    ensure_sync_project_task, AgentRef, EnsureSyncProjectTaskResult, WorkItem, WorkItemTree,
    WorkTreeSnapshot, SYNC_PROJECT_TASK_TITLE,
};

// Draft workspace management
pub use draft::{
    create_workspace_provider, FileEntry, LocalWorkspaceProvider, StartingPoint, WorkspaceInfo,
    WorkspaceProvider,
};

// GitHub client
pub use github::{GitHubClient, GitHubError, GitHubResult, MergeInfo, PrInfo};

// Dispatch service
pub use dispatch::{
    DispatchConfig, DispatchError, DispatchInfo, DispatchResult as DispatchServiceResult,
    DispatchService,
};

// Delivery service
pub use delivery::{
    delivery_branch_name, pr_body, pr_title, DeliveryError, DeliveryOrchestrator, DeliveryResult,
    DeliveryState, DeliveryStatus, GitOperations, PushResult,
};

// Forge providers
pub use forge::{
    create_forge_for_remote, ForgeError, ForgeProvider, ForgeResult, GitHubForge,
    MergeResult as ForgeMergeResult, PrInfo as ForgePrInfo,
};

// Board service (tree operations and agent file sync)
pub use board::{
    BoardError, BoardJson, BoardResult, BoardService, BoardSnapshot, BoardStorage,
    Bookmark as BoardBookmark, CreateEvalRequest, CreateTaskRequest, DispatchPreview,
    Eval as BoardEval, EvalStatus as BoardEvalStatus, ExportScope, LocalBoardStorage,
    RemoteBoardStorage, SyncResult as BoardSyncResult, Task as BoardTask, TaskFile,
    TaskRun as BoardTaskRun, TaskStatus as BoardTaskStatus, TaskTree, UpdateEvalRequest,
    UpdateTaskRequest,
};
pub use capabilities::CapabilityProfile;

// Delta dispatch (unified board tree)
pub use delta::{
    BoardNode, BoardNodeDifficulty, BoardNodeSource, BoardNodeStatus, BoardNodeTree, BoardVersion,
    CreateBoardNodeRequest, DeltaDispatchService, DeltaState,
    DispatchResult as DeltaDispatchResult, NodeKind, RouteRuntime, RouteRuntimeStatus,
    UpdateBoardNodeRequest,
};

// Route management
pub use route::{
    CreateRouteRequest, Route, RouteError, RouteFiles, RouteResult, RouteStore, RouteTree,
};
pub use route_runtime::{
    ensure_route_runtime, get_route_runtime_name, runtime_name_for_route, RouteRuntimeHandle,
};
