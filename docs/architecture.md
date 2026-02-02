# Hirsel Architecture Reference

> **Keep this document updated** when modifying core modules.

## Quick Reference

### Common Modification Points

| Task | Files to Modify |
|------|-----------------|
| Add CLI command | `src-tauri/src/cli/mod.rs` → `Commands` enum, new `src-tauri/src/cli/<cmd>.rs` |
| Add GUI command | `src-tauri/src/gui/commands/mod.rs` → `get_handlers()`, new handler in relevant submodule |
| Add REST endpoint | `src-tauri/src/core/server/mod.rs` → router, `src-tauri/src/core/server/routes.rs` |
| Modify run state | `src-tauri/src/core/state/mod.rs` → `SCHEMA` const, `src-tauri/src/core/state/types.rs` |
| Add runner type | `src-tauri/src/core/runner/mod.rs` → `create_runner()`, new `src-tauri/src/core/runner/<type>.rs` |
| Modify lifecycle | `src-tauri/src/core/lifecycle/mod.rs`, `src-tauri/src/core/lifecycle/local.rs` |
| Add archive strategy | `src-tauri/src/core/snapshot/mod.rs`, new strategy impl of `ArchiveStrategy` |
| Add service worker | `src-tauri/src/core/service_worker/scribe.rs`, `src-tauri/src/cli/service_worker.rs` |
| Add board task/eval | `src-tauri/src/core/board/mod.rs`, `src-tauri/src/gui/commands/specflow.rs` |
| Modify board UI | `src/components/specflow/SpecBoard.tsx` |
| Add orchestrator method | `src-tauri/src/core/orchestrator/mod.rs` → trait, `local.rs`, `daemon.rs`, `remote.rs` impls |
| Modify delta dispatch | `src-tauri/src/core/delta/runner.rs`, `src-tauri/src/daemon/lifecycle.rs` |

### Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `gui` | Tauri desktop app (includes `cli`) | Yes |
| `cli` | Full CLI (includes `server` + TUI attach) | No (implied by `gui`) |
| `server` | HTTP server, daemon | No (implied by `cli`) |
| `worker` | Minimal remote worker binary | No |
| `s3-storage` | S3-compatible storage backend | No |

```bash
cargo build                                     # Full GUI
cargo build --no-default-features -F cli        # CLI only
cargo build --no-default-features -F worker     # Remote worker
cargo build --features s3-storage               # With S3 support
```

---

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Interface                            │
├──────────────────────┬──────────────────────┬───────────────────┤
│   CLI (hirsel)       │   GUI (Tauri)        │   HTTP Server     │
└──────────┬───────────┴──────────┬───────────┴─────────┬─────────┘
           │                      │                     │
           ▼                      ▼                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Daemon (background process)                   │
│   - Lifecycle polling, eval triggering, time limits             │
│   - Worker spawning via Orchestrator                            │
│   - Auto-exit when idle                                         │
└──────────────────────────────┬──────────────────────────────────┘
                               │
           ┌───────────────────┼───────────────────┐
           ▼                   ▼                   ▼
    LocalOrchestrator   DaemonOrchestrator   RemoteOrchestrator
    (direct access)     (TCP)                (HTTP API)
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│   SQLite State              │   Worker Processes                 │
│   (runs, workers, tasks,    │   (Local, SSH, Fly)                │
│    messages, events)        │   + optional Docker container      │
└─────────────────────────────┴───────────────────────────────────┘
```

---

## Module Map

### `src-tauri/src/core/` - Core Business Logic

| Submodule | Key Files | Purpose |
|-----------|-----------|---------|
| `state/` | `mod.rs`, `types.rs`, `run.rs`, `workers.rs`, `messages.rs`, `events.rs`, `evals.rs`, `history.rs`, `scribe.rs` | SQLite state management (per-run) |
| `project/` | `mod.rs`, `types.rs`, `store.rs` | Project database (global), SpecFlow per-project configuration |
| `board/` | `mod.rs`, `types.rs`, `storage.rs` | SpecFlow board data (tasks, evals, task tree, file sync) |
| `github/` | `mod.rs` | GitHub API client (octocrab) with auth fallback (env → gh config → hirsel config) |
| `dispatch/` | `mod.rs` | Dispatch service: creates runs from board tasks, generates spec/eval, creates work+eval tasks with validates relationship |
| `delta/` | `mod.rs`, `runner.rs`, `state.rs`, `types.rs` | Delta dispatch system: draft/live tree diffs, persistent project runs, live_nodes |
| `delivery/` | `mod.rs` | Delivery service: three-tier delivery (push/PR/merge), conflict detection, staleness checking |
| `orchestrator/` | `mod.rs` → `Orchestrator` trait, `local.rs`, `remote.rs`, `daemon.rs` | Run orchestration pattern |
| `lifecycle/` | `mod.rs` → `LifecycleManager` trait, `local.rs`, `remote.rs`, `transitions.rs` | Event-driven state machine |
| `runner/` | `types.rs` → `Runner` trait, `local.rs`, `fly.rs`, `ssh.rs`, `composed.rs`, `config.rs`, `setup.rs` | Worker host implementations |
| `run_manager/` | `mod.rs`, `local.rs`, `remote.rs` | Unified run management wrapping Orchestrator + Lifecycle |
| `chat_orchestrator/` | `mod.rs` → `ChatOrchestrator` trait, `local.rs`, `remote.rs` | Chat session orchestration (local/remote) |
| `snapshot/` | `mod.rs`, `archive.rs`, `noop.rs`, `s3.rs`, `claude_session.rs` | Work/session persistence |
| `draft/` | `mod.rs`, `types.rs` → `StartingPoint`, `workspace.rs`, `local_workspace.rs`, `s3_workspace.rs` | StartingPoint, workspace init |
| `config/` | `mod.rs`, `store.rs`, `loader.rs`, `saver.rs`, `types.rs`, `agent.rs`, `storage.rs`, `orchestrator.rs`, `paths.rs` | Config struct, DB storage, profiles, runners |
| `ops/` | `mod.rs`, `run.rs`, `setup.rs`, `spawn.rs`, `project.rs`, `docs.rs`, `types.rs` | Shared CLI/GUI operations |
| `server/` | `mod.rs` → `start_server()`, `routes.rs`, `auth.rs`, `gyp.rs`, `board.rs` | HTTP server for remote mode |
| `eval/` | `mod.rs`, `acp.rs`, `context.rs`, `parser.rs`, `script.rs`, `types.rs` | Eval runner: ACP eval agent, context building, script parsing |
| `storage/` | `mod.rs` | File storage abstraction (local/S3) |
| `service_worker/` | `mod.rs`, `scribe.rs`, `conflict_resolver.rs`, `types.rs` | Service workers: ScribeService for documentation, ConflictResolverServiceWrapper for merge conflicts |
| `conflict_resolver/` | `mod.rs`, `client.rs`, `state.rs` | Git conflict resolution with AI agent |
| `acp_runner.rs` | - | Unified ACP agent runner for scribe, conflict_resolver, compaction, eval |
| `error.rs` | - | `HirselError` enum with `ErrorKind` categorization |
| `acp.rs` | - | Agent Control Protocol types, `AcpChild` process wrapper |
| `state_access.rs` | - | Worker state abstraction (SQLite vs HTTP) |
| `chat_session.rs` | - | Chat session management with event channels |
| `metrics.rs` | - | Session metrics extraction with TTL cache |
| `files.rs` | - | Run directory file operations |
| `chats.rs` | - | GypChat message storage |
| `gyp_chat.rs` | - | Project-level chat history |
| `gyp_context.rs` | - | Gyp context building |
| `project_messages.rs` | - | Sheepfold: project-scoped messaging (Meadow group chat + worker DMs) |
| `api_types.rs` | - | Shared API response types |
| `worker_routes.rs` | - | Worker HTTP handlers |
| `message_routes.rs` | - | Message HTTP handlers |
| `eval_routes.rs` | - | Eval HTTP handlers |
| `git_http.rs` | - | Git HTTP server for remote workers |
| `credentials.rs` | - | Encrypted credential store |
| `git.rs` | - | Git operations |
| `tailscale.rs` | - | Tailscale integration |
| `gyp.rs` | - | Unified Gyp context builder and session config |

### `src-tauri/src/worker/` - Worker Subprocess

| File | Purpose |
|------|---------|
| `acp_client.rs` | ACP connection, message handling, prompt building |
| `runner.rs` | Worker execution loop (`WorkerRunner`), eval_pass/eval_fail handlers |
| `msg.rs` | Message types and serialization |
| `mcp.rs` | MCP server for worker tools (including eval_pass, eval_fail) |
| `eval_mcp.rs` | MCP server for legacy eval tools |
| `remote_runner.rs` | Remote worker entry point |
| `http_state.rs` | HTTP-based state for remote workers |
| `file_server.rs` | File upload server for remote workers |

**MCP Worker Tools** (available to all workers):
- Live Node Management: `get_task_tree`, `get_available_tasks`, `get_my_tasks`, `get_task_details`, `complete_task`, `add_task`, `add_eval`
- Communication: `list_contacts`, `chat_history`, `chat_send`, `chat_unread`
- Documentation: `scribe`, `read_docs`
- Work Management: `work_done` (signal ready for next task), `time_status`

**Note:** These tools operate on **live_nodes** in the global database (`~/.hirsel/hirsel.db`), not the old per-run SQLiteState tasks.

**MCP Eval Tools** (available to eval tasks):
- `eval_pass` - Mark eval as passed, validate all tasks in `validates[]`
- `eval_fail(feedback)` - Mark eval as failed, create repair task as child of eval

**Note:** Workers receive pre-assigned tasks at spawn time. There is no `claim_task` tool - task assignment is handled by the coordinator via direct assignment.

### `src-tauri/src/gui/commands/` - Tauri IPC Commands

| File | Commands |
|------|----------|
| `runs.rs` | `get_runs`, `get_run_detail`, `pause_run`, `resume_run`, `delete_run`, `delete_all_runs`, `deliver_run` |
| `drafts.rs` | `validate_repo`, `create_draft`, `clone_run`, `update_draft`, `start_draft`, `change_starting_point` |
| `workers.rs` | `get_workers`, `attach_worker`, `open_worker_terminal`, `detach_worker`, `restart_worker` |
| `messages.rs` | `get_messages`, `get_threads`, `get_all_unread_notifications`, `send_message`, `mark_messages_read` |
| `events.rs` | `get_worker_events`, `clear_worker_events`, `start_worker_event_stream`, `stop_worker_event_stream` |
| `chat.rs` | `start_chat_session`, `send_chat_message`, `respond_chat_permission`, `stop_chat_session`, `list_chat_sessions` |
| `config_cmd.rs` | `get_config`, `save_config`, `get_tailscale_info`, `check_ssh_runner` |
| `credentials.rs` | `store_credential`, `delete_credential`, `has_credential`, `get_credential`, `get_credential_masked` |
| `files.rs` | `read_spec_file`, `write_spec_file`, `read_eval_file`, `write_eval_file`, `save_asset`, `import_asset_from_path`, `open_assets_folder`, `get_assets_path` |
| `logs.rs` | `get_eval_log`, `get_eval_log_by_path`, `get_history`, `get_eval_spec`, `get_evals` |
| `filesystem.rs` | `pick_folder`, `suggest_paths` |
| `debug.rs` | `log_frontend`, `get_version`, `get_process_counts`, `kill_orphaned_acp_processes`, `get_gyp_chat_history`, `save_gyp_message`, `clear_gyp_chat_history` |
| `projects.rs` | `list_projects`, `get_project`, `create_project_from_path`, `delete_project` |
| `gyp.rs` | `start_gyp_session`, `send_gyp_message`, `save_gyp_message`, `get_gyp_history`, `clear_gyp_history`, `stop_gyp_session` |
| `delta.rs` | `get_draft_tree`, `get_live_tree`, `create_draft_node`, `update_draft_node`, `delete_draft_node`, `move_draft_node`, `reset_project_tree`, `compute_tree_diff`, `get_diff_summary`, `dispatch_deltas`, `preview_delta_dispatch`, `get_project_run`, `complete_live_node`, `complete_revert`, `get_dual_trees`, `sync_gyp_changes` |
| `delivery.rs` | `get_delivery_state`, `check_merge_state`, `get_conflicting_files`, `check_staleness`, `push_run_branch`, `create_run_pr`, `auto_merge_run`, `generate_pr_title`, `generate_pr_body`, `delivery_branch_name`, `get_board_versions`, `get_latest_board_version`, `get_current_board_delivery`, `start_board_delivery`, `get_board_delivery_status`, `retry_board_delivery`, `get_delivery_attempts`, `complete_board_delivery`, `abandon_board_delivery` |

### `src-tauri/src/cli/` - CLI Commands

| File | Command | Feature |
|------|---------|---------|
| `go.rs` | `hirsel go <run> <spec>` | `cli` |
| `runs.rs` | `hirsel runs` | - |
| `view.rs` | `hirsel view <run>` | - |
| `log.rs` | `hirsel log <run>` | - |
| `attach.rs` | `hirsel attach <run>` | `cli` |
| `pause.rs` | `hirsel pause <run>` | - |
| `resume.rs` | `hirsel resume <run>` | - |
| `delete.rs` | `hirsel delete <run>` | - |
| `deliver.rs` | `hirsel deliver <run>` | - |
| `msg.rs` | `hirsel msg <run>` | - |
| `tasks.rs` | `hirsel tasks <project>` - View live nodes for a project | - |
| `diff.rs` | `hirsel diff <run>` | - |
| `summary.rs` | `hirsel summary <run>` | - |
| `spec.rs` | `hirsel spec <run>` | - |
| `asset.rs` | `hirsel asset <run>` | - |
| `config.rs` | `hirsel config` | - |
| `prune.rs` | `hirsel prune` | - |
| `reset.rs` | `hirsel reset` | - |
| `templates.rs` | `hirsel templates` | - |
| `man.rs` | `hirsel man` | - |
| `completions.rs` | `hirsel completions` | - |
| `test.rs` | `hirsel test <scenario>` | `cli` |
| `scribe.rs` | `hirsel scribe <run>` | - |
| `helpers.rs` | Shared helper functions | - |
| `tui.rs` | Terminal UI for `attach` command | `cli` |
| `mod.rs` | `hirsel mode <run>`, `hirsel amend <run>` (inline) | - |
| `acp_bridge.rs` | `hirsel __acp-bridge` | - |
| `service_worker.rs` | `hirsel __service-worker --type scribe` | `cli` |
| `mod.rs` | `hirsel clone <run>` (inline) | - |
| `mod.rs` | `hirsel serve` (inline) | `server` |

### `src-tauri/src/daemon/` - Background Process

| File | Purpose |
|------|---------|
| `mod.rs` | Socket/PID paths, `is_daemon_running()` |
| `server.rs` | Daemon server, TCP listener |
| `lifecycle.rs` | Polling loop, lifecycle action handling |
| `client.rs` | Client for daemon communication |

### `src/` - Frontend (SolidJS)

| Directory | Purpose |
|-----------|---------|
| `components/layout/` | Layout, TitleBar, StatusBar |
| `components/runs/` | RunListPanel, RunDetail, WorkerCard, TaskTreeView |
| `components/specflow/` | SpecBoard (unified canvas component with delta dispatch, node rendering, context menus) |
| `components/modals/` | SettingsModal, HelpModal, ConfirmDialog |
| `components/chat/` | GypMessenger |
| `stores/` | AppProvider, ProjectProvider, RunsProvider, SelectionProvider, DeltaProvider |
| `hooks/` | usePolling, useDebounce, useTauriEvent |
| `lib/` | Icons, theme, toast, dev-logger, utils |
| `lib/elk-layout.ts` | ELK.js wrapper for hierarchical graph layout with orthogonal edge routing |

**SpecBoard Architecture:**
- `SpecBoard.tsx` - Unified canvas component handling:
  - Draft/live tree visualization with delta dispatch
  - Node rendering with visual hierarchy indicators
  - Context menus for node operations
  - Drag-and-drop node reordering
  - Keyboard navigation and shortcuts
- **Graph Layout** - ELK.js (Eclipse Layout Kernel) for hierarchical graph layout:
  - `src/lib/elk-layout.ts` - ELK wrapper with orthogonal edge routing
  - Layered algorithm with proper crossing minimization
  - Uses only tree structure (parent-child) for positioning
  - blockedBy/validates edges are visual overlays that don't affect layout
- **DependencyConnectors** - SVG polylines for dependency edges:
  - `validates` (eval→task): sage green lines
  - `blockedBy` (task→task): terra red lines
  - `blockedBy` computed as inverse of `validates` in backend tree builders

---

## Key Traits

### `Orchestrator` (`src-tauri/src/core/orchestrator/mod.rs`)

High-level run management interface. CLI, GUI, and server use this trait.

```rust
pub trait Orchestrator: Send + Sync {
    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>>;
    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail>;
    async fn delete_run(&self, name: &str) -> OrchestratorResult<()>;
    async fn pause_run(&self, name: &str) -> OrchestratorResult<()>;
    async fn resume_run(&self, name: &str, time_limit: Option<u32>) -> OrchestratorResult<()>;
    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>>;
    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail>;
    async fn init_workspace(&self, run: &str, request: InitWorkspaceRequest) -> OrchestratorResult<InitWorkspaceResponse>;
    async fn spawn_single_worker(&self, run: &str, worker: &str, work_dir: &Path, session_id: Option<&str>) -> OrchestratorResult<()>;
    async fn resume_worker(&self, run: &str, worker: &str, work_dir: &Path, session_id: Option<&str>, state: Option<&WorkerStateHandle>) -> OrchestratorResult<()>;
    // ... more methods
}
```

**Note:** Task management has been removed from the Orchestrator trait. Workers now interact with live_nodes directly via StateAccess methods.

| Implementation | Location | Use Case |
|----------------|----------|----------|
| `LocalOrchestrator` | `orchestrator/local.rs` | Direct SQLite access (daemon, server) |
| `DaemonOrchestrator` | `orchestrator/daemon.rs` | TCP to daemon (CLI/GUI local mode) |
| `RemoteOrchestrator` | `orchestrator/remote.rs` | HTTP API to remote server |

### `LifecycleManager` (`src-tauri/src/core/lifecycle/mod.rs`)

Centralized lifecycle state machine. Returns actions for daemon to execute.

```rust
pub trait LifecycleManager {
    fn process_event(&self, event: LifecycleEvent) -> LifecycleResult<Vec<LifecycleAction>>;
    fn pause_run(&self, reason: &str) -> LifecycleResult<Vec<String>>;
    fn resume_run(&self) -> LifecycleResult<Vec<LifecycleAction>>;
    fn worker_done(&self, worker_name: &str) -> LifecycleResult<Vec<LifecycleAction>>;
    fn should_trigger_eval(&self) -> LifecycleResult<bool>;
    fn run_status(&self) -> LifecycleResult<Status>;
}
```

| Event | Action(s) |
|-------|-----------|
| `TimeCheck` | `EvalTriggered`, `RunFailed`, `TimeWarning` |
| `WorkerDone` | `EvalTriggered`, `RunCompleted` |
| `PauseRequested` | `WorkersPaused`, `RunStatusChanged` |
| `ResumeRequested` | (handled via scaling check) |
| `ScalingCheck` | `SpawnWorker(assigned_task_id)`, `WakeWorker(assigned_task_id)` |

| Implementation | Location | Use Case |
|----------------|----------|----------|
| `LocalLifecycleManager` | `lifecycle/local.rs` | Local/daemon mode |
| `RemoteLifecycleManager` | `lifecycle/remote.rs` | Remote workers (delegates to coordinator) |

### `Runner` (`src-tauri/src/core/runner/types.rs`)

Worker spawning interface. Host + optional Container model.

```rust
#[async_trait]
pub trait Runner: Send + Sync {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult>;
    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()>;
    async fn is_alive(&self, handle: &WorkerHandle) -> bool;
    fn runner_type(&self) -> &'static str;
    async fn setup(&self) -> RunnerResult<()>;
    async fn cleanup(&self) -> RunnerResult<()>;
}

// WorkerSpawnConfig includes assigned_task_id for direct task assignment
pub struct WorkerSpawnConfig {
    pub worker_name: String,
    pub work_dir: PathBuf,
    pub assigned_task_id: Option<String>,  // Pre-assigned task
    // ... other fields
}
```

| Implementation | Location | Host Type | Ephemeral |
|----------------|----------|-----------|-----------|
| `LocalRunner` | `runner/local.rs` | Local machine | No |
| `SshRunner` | `runner/ssh.rs` | Remote via SSH | No |
| `FlyRunner` | `runner/fly.rs` | Fly.io machine | Yes |
| `ComposedRunner` | `runner/composed.rs` | Executor + Resource | Varies |

### `ArchiveStrategy` (`src-tauri/src/core/snapshot/archive.rs`)

Unified directory archiving for pause/resume. Replaces the separate `SnapshotStrategy` and `AgentSessionStorage` traits.

```rust
#[async_trait]
pub trait ArchiveStrategy: Send + Sync {
    async fn archive(&self, key: &str, source_dir: &Path) -> ArchiveResult<ArchiveHandle>;
    async fn restore(&self, handle: &ArchiveHandle, target_dir: &Path) -> ArchiveResult<()>;
    async fn delete(&self, handle: &ArchiveHandle) -> ArchiveResult<()>;
    fn strategy_type(&self) -> &'static str;
}
```

| Implementation | Location | Host Types | Requirement |
|----------------|----------|------------|-------------|
| `NoOpArchiveStrategy` | `snapshot/noop.rs` | Local, SSH, Client | None (files persist on disk) |
| `S3ArchiveStrategy` | `snapshot/s3.rs` | Fly | `s3-storage` feature |

### `WorkspaceProvider` (`src-tauri/src/core/draft/workspace.rs`)

Workspace initialization from StartingPoint.

```rust
#[async_trait]
pub trait WorkspaceProvider: Send + Sync {
    async fn init(&self, run_name: &str, starting_point: &StartingPoint) -> Result<WorkspaceInfo>;
    fn workspace_path(&self, run_name: &str) -> PathBuf;
}
```

| Implementation | Location | Storage |
|----------------|----------|---------|
| `LocalWorkspaceProvider` | `draft/local_workspace.rs` | Filesystem |
| `S3WorkspaceProvider` | `draft/s3_workspace.rs` | S3 (feature-gated) |

---

## Cross-Cutting Patterns

### Error Handling (`src-tauri/src/core/error.rs`)

Unified error hierarchy for the codebase.

- `HirselError` enum with variants for all error types
- `ErrorKind` for categorization: `NotFound`, `AlreadyExists`, `InvalidState`, `InvalidInput`, `State`, `Io`, `Git`, `Network`, `Auth`, `Process`, `Serialization`, `Timeout`, `Internal`
- Automatic HTTP status mapping via `http_status()` method
- `is_user_error()` distinguishes user errors from system errors

### Process Management (`src-tauri/src/core/acp.rs`)

`AcpChild` wraps subprocess lifecycle for clean process management.

```rust
pub struct AcpChild {
    child: Child,
    context: String,
    pid: Option<u32>,
}
```

- Creates process groups on Unix for proper cleanup of child processes
- Drop impl: SIGTERM → wait with timeout → SIGKILL
- Used for workers, chat sessions, ACP bridge

### State Access Abstraction (`src-tauri/src/core/state_access.rs`)

Workers transparently use local (SQLite) or remote (HTTP) state.

```rust
#[async_trait(?Send)]
pub trait StateAccess: Send {
    // Run status
    async fn status(&self) -> StateAccessResult<Status>;
    async fn set_status(&self, status: Status) -> StateAccessResult<()>;

    // Live nodes (work items from delta dispatch)
    async fn add_live_node(&self, ...) -> StateAccessResult<()>;
    async fn claim_live_node(&self, id: &str, worker_name: &str) -> StateAccessResult<()>;
    async fn complete_live_node(&self, id: &str, worker_name: &str) -> StateAccessResult<()>;
    async fn get_claimed_live_node(&self, worker_name: &str) -> StateAccessResult<Option<LiveNode>>;
    async fn get_claimable_nodes(&self) -> StateAccessResult<Vec<LiveNode>>;
    async fn live_node_eval_pass(&self, eval_id: &str, worker_name: &str) -> StateAccessResult<()>;
    async fn live_node_eval_fail(&self, eval_id: &str, worker_name: &str, feedback: &str) -> StateAccessResult<String>;

    // Scaling
    async fn request_scaling_check(&self) -> StateAccessResult<()>;
    // ... message, worker operations
}
```

- Workers use `HIRSEL_API_URL` environment variable to determine mode
- Enables same worker binary for local and remote deployment
- `SQLiteState` for local, `HttpState` for remote
- Live nodes are stored in global database (`~/.hirsel/hirsel.db`), not per-run
- `request_scaling_check` triggers event-driven worker scaling (via DB flag or HTTP)

### Board Service (`src-tauri/src/core/board/mod.rs`)

Transparent local/remote routing for board file sync with AI agents.

```rust
pub struct BoardService {
    project_id: i64,
}

impl BoardService {
    pub fn export_local_sync(&self) -> Result<PathBuf>; // Export board to JSON files
    pub fn import_local_sync(&self) -> Result<SyncResult>; // Import changes from JSON
    pub fn board_dir(&self) -> PathBuf; // Get board directory path
}
```

- Exports islands as individual JSON files in `~/.hirsel/projects/{id}/board/islands/`
- Enables AI agents to read/modify board state via file system
- Import syncs JSON changes back to SQLite database
- Used by Gyp AI context integration (deferred)

---

## Real-Time Events

### SSE Streaming (`src-tauri/src/core/server/gyp.rs`)

Server-Sent Events for chat/worker updates.

- Channel-based: `tokio::sync::mpsc::UnboundedChannel`
- Event types: `TextDelta`, `ToolCallStart`, `ToolCallDelta`, `ToolCallComplete`, `PermissionRequest`, `SessionComplete`, `Error`
- Endpoint: `/api/gyp/sessions/{id}/events`
- GypState manages active sessions with cleanup on disconnect

### ChatOrchestrator Trait (`src-tauri/src/core/chat_orchestrator/mod.rs`)

Mirrors Orchestrator pattern for chat-specific operations.

```rust
#[async_trait]
pub trait ChatOrchestrator: Send + Sync {
    async fn start_session(&self, context: ChatContext) -> ChatOrchestratorResult<String>;
    async fn stop_session(&self, session_id: &str) -> ChatOrchestratorResult<()>;
    async fn send_message(&self, session_id: &str, message: &str) -> ChatOrchestratorResult<()>;
    async fn respond_permission(&self, session_id: &str, response: PermissionResponse) -> ChatOrchestratorResult<()>;
    async fn list_sessions(&self) -> ChatOrchestratorResult<Vec<SessionInfo>>;
    fn subscribe(&self, session_id: &str) -> ChatOrchestratorResult<BoxStream<'static, ChatEvent>>;
}
```

| Implementation | Location | Use Case |
|----------------|----------|----------|
| `LocalChatOrchestrator` | `chat_orchestrator/local.rs` | Direct in-process |
| `RemoteChatOrchestrator` | `chat_orchestrator/remote.rs` | HTTP + SSE to coordinator |

---

## State Machine

### Run Status (`src-tauri/src/core/state/types.rs`)

```
Draft ──start──► Working ───────────────────────► Done ──deliver──► Delivered
                    │                                │
                    │ (all work+eval tasks done)     │
                  pause                              │
                    │                                │
                    ▼                                ▼
                 Paused              Failed (time limit, manual, etc.)
                    │
                  resume
                    │
                    ▼
                 Working
```

**Note:** Run stays in `Working` throughout both work and eval task execution. Eval tasks are regular tasks in the same worker pool - there is no separate "Eval" run status in the unified model.

| Status | Description | Terminal |
|--------|-------------|----------|
| `Draft` | Configured, workers not spawned | No |
| `Working` | Workers actively running (both work and eval tasks) | No |
| `Paused` | Manually paused by user | No |
| `Eval` | Legacy: Evaluation in progress (deprecated in unified model) | No |
| `Done` | Completed successfully (all work tasks validated) | Yes |
| `Delivered` | Changes pushed to branch | Yes |
| `Failed` | Run failed (see `failure_reason`) | Yes |

### Worker Status (`src-tauri/src/core/state/types.rs`)

| Status | Description |
|--------|-------------|
| `Working` | Actively processing |
| `Awaiting` | Idle (no work or waiting for user) |
| `Paused` | Stopped (run is paused) |
| `Error` | Process died unexpectedly |

### Live Node Status (`src-tauri/src/core/delta/types.rs`)

Live nodes are the work items in the delta dispatch system, stored in the global database.

| Status | Description |
|--------|-------------|
| `Pending` | Not started, waiting to be claimed |
| `Working` | Claimed by worker, in progress |
| `Done` | Completed successfully |
| `Failed` | Failed (eval failed or error) |

### Live Node Type (`src-tauri/src/core/delta/types.rs`)

| Type | Description |
|------|-------------|
| `Task` | Implementation task that produces code changes |
| `Eval` | Task that validates other tasks |

### Live Node Source (`src-tauri/src/core/delta/types.rs`)

| Source | Description |
|--------|-------------|
| `Spec` | From dispatch (created from draft nodes) |
| `Worker` | Added by worker via MCP |
| `System` | System nodes (scope) |

### Live Node Lifecycle

```
TASK:  Pending → Working → Done
                    ↓ (blocked by eval failure)
                  Failed → (repaired) → Pending

EVAL:  Pending → Working → Done (pass) or Failed (fail)
                              ↓ (if failed)
                           creates repair task
```

Workers claim and complete live_nodes via MCP tools. When an eval passes, its `validates` nodes are unblocked. When an eval fails, a repair task may be created.

---

## Data Flow

### SpecFlow Board Dispatch

```
SpecFlow Board (GUI)
   │
   ▼ User selects rows → Enter dispatch mode
Toggle row selection
   │
   ▼ dispatch_rows(projectId, rowIds)
specflow.rs command
   │
   ├─► Expand selection with blocked_by dependencies (ripple)
   ├─► Check for already-dispatched rows → Warning if found
   ├─► Sort rows by island Y position, then row position
   │
   ▼ Generate run inputs
┌─────────────────────────────────────────────────────────┐
│ 1. Generate spec.md from row spec_content (by island)   │
│ 2. Generate eval.md from row eval_criterion (by island) │
│ 3. Extract initial tasks from row task_title/desc       │
│ 4. Create draft run via create_draft()                  │
│ 5. Mark rows as dispatched, store run_name              │
└─────────────────────────────────────────────────────────┘
   │
   ▼ User edits draft, starts run
Standard run lifecycle
   │
   ▼ sync_run_status(projectId, runName)
Update board rows from run task statuses
```

### Run Creation (Local Mode)

```
CLI/GUI
   │
   ▼ StartRunRequest
DaemonOrchestrator ──TCP──► Daemon
                                      │
                                      ▼ start_run_internal
                                LocalOrchestrator
                                      │
   ┌──────────────────────────────────┴──────────────────────────────────┐
   │ 1. Create run directory (~/.hirsel/runs/<name>/)                    │
   │ 2. Initialize SQLite database (hirsel.db)                           │
   │ 3. Write spec.md from request                                       │
   │ 4. Set up workspace via WorkspaceProvider (git worktrees)           │
   │ 5. Register workers in state                                        │
   │ 6. Spawn workers via Runner (unless draft mode)                     │
   └─────────────────────────────────────────────────────────────────────┘
```

### Worker Lifecycle (Event-Driven)

Workers receive pre-assigned tasks at spawn time. No worker-initiated task claiming.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Event-Driven Scaling                              │
│                                                                      │
│  Task state change ──► request_scaling_check() ──► DB flag set      │
│                                                                      │
│  Daemon 5s poll ──► consume_scaling_check() ──► evaluate_scaling()  │
│       │                                                              │
│       └──► SpawnWorker(assigned_task_id) or WakeWorker(...)         │
└─────────────────────────────────────────────────────────────────────┘

Daemon ──spawn(assigned_task_id)──► Worker Process
                                          │
                                          ▼
                                   AcpClient.connect()
                                          │
                                          ▼
                                   Send prompt with assigned task
                                          │
                                          ▼
                                  ┌───────────────┐
                                  │  Main Loop    │◄─────────────────┐
                                  └───────┬───────┘                  │
                                          │                          │
                                          ▼                          │
                                  Process agent response             │
                                  (text, tool calls)                 │
                                          │                          │
                                          ▼                          │
                                  Update state (events, status)      │
                                          │                          │
                                          ▼                          │
                                  Check for messages ────────────────┘
                                          │
                                          ▼ (task complete)
                                  complete_task() + work_done()
                                          │
                                          ▼
                                  request_scaling_check()
                                          │
                                          ▼
                                  Worker exits (fresh context per task)
                                          │
                                          ▼
                                  Daemon evaluates scaling
                                          │
                                          ▼
                                  Respawn with new task (if available)
```

**Key behaviors:**
- **Direct task assignment**: Workers spawn with `assigned_task_id`, no `claim_task` tool
- **Fresh context**: Each task gets a new worker session (no session resume)
- **Scope blocking**: Root tasks are blocked by "scope" task until leader explores spec
- **Tree-walk distance**: Task assignment prefers nearby tasks (same subtree) for work, distant for eval

### Archive/Restore (Ephemeral Runners)

**Pause:**
```
Daemon.pause_run()
   │
   ▼
LocalLifecycleManager.pause_run()
   │
   ├─► For each worker:
   │      1. ArchiveStrategy.archive() ──► ArchiveHandle (work dir)
   │      2. ArchiveStrategy.archive() ──► ArchiveHandle (session)
   │      3. Store handles in WorkerStateHandle, save to worker DB
   │      4. Runner.stop()
   │
   └─► Set run status = Paused
```

**Resume:**
```
Daemon.resume_run()
   │
   ▼
LocalLifecycleManager.resume_run()
   │
   └─► Triggers scaling check
         │
         ▼
Daemon evaluates scaling:
   1. Check if runner is_ephemeral()
   2. If yes: ArchiveStrategy.restore(work_dir_handle)
   3. Runner.spawn() with assigned_task_id (fresh context)
   4. Update worker DB (clear state handle, set status)
```

**Note:** Workers always spawn with fresh context. Session resume is not used - each task gets a new agent session for cleaner context management.

---

## Database Schema

### Global Database (`~/.hirsel/hirsel.db`)

| Table | Primary Key | Purpose |
|-------|-------------|---------|
| `config` | `key` | Configuration key-value store |
| `credentials` | `key_type` | Encrypted credential storage |
| `gyp_chat_messages` | `id` | GYP chat history |
| `projects` | `id` | Project registry |
| `draft_nodes` | `id` | Draft tree nodes (user's editable spec) |
| `live_nodes` | `id` | Live tree nodes (dispatched work items) |
| `delta_submissions` | `id` | Pending delta batches |
| `project_runs` | `id` | Persistent project runs |
| `board_versions` | `id` | Board version history |
| `board_deliveries` | `id` | Board delivery tracking |
| `project_messages` | `id` | Sheepfold messages |

**config:**
- `key` - Configuration key (e.g., "runners", "auth", "eval_timeout")
- `value` - JSON or string value
- `updated_at` - Last modification timestamp

**live_nodes** (work items for delta dispatch):
- `id` - Unique node ID (slug)
- `project_id` - Parent project
- `draft_node_id` - Corresponding draft node (if any)
- `parent_id` - Parent live node
- `name`, `content` - Node details
- `node_type` - 'task' or 'eval'
- `status` - 'pending', 'working', 'done', 'failed'
- `source` - 'spec', 'worker', 'system'
- `validates` - JSON array of task IDs this eval validates
- `blocked_by` - JSON array of blocking node IDs

### Run Database (`~/.hirsel/runs/{name}/hirsel.db`)

| Table | Primary Key | Purpose |
|-------|-------------|---------|
| `state` | `id=1` | Run metadata (singleton) |
| `workers` | `id` | Worker processes |
| `messages` | `id` | Chat threads |
| `message_reads` | `(worker_name, thread)` | Read tracking |
| `worker_events` | `id` | Real-time output streaming |
| `evals` | `id` | Evaluation runs |
| `history` | `id` | Activity log |
| `amendments` | `id` | Spec amendments |

**Note:** Work items (tasks) are stored as **live_nodes** in the global database, not per-run. See Global Database section.

### Key Columns

**state:**
- `status`, `failure_reason`, `started_at`, `time_limit_minutes`
- `worker_scale`, `human_in_the_loop`
- `default_runner`, `worker_runners` (JSON), `runner_configs` (JSON), `starting_point` (JSON)
- `scaling_check_requested` - Boolean flag for event-driven scaling

**workers:**
- `name`, `pid`, `runner_id`, `runner_type`, `status`
- `session_id`, `work_dir`, `hitl_waiting`
- `state_handle` (JSON: WorkerStateHandle with work_dir and agent_session snapshots)
- `assigned_task_id` - Currently assigned live_node (direct assignment model)
- `last_task_id` - Last completed live_node (for tree-walk distance calculation)

### Project Board Tables (in global DB)

The board module stores project-level data in the global database (`~/.hirsel/hirsel.db`):

| Table | Primary Key | Purpose |
|-------|-------------|---------|
| `board_tasks` | `id` | Board tasks (spec planning tree) |
| `board_evals` | `id` | Board evals (validation criteria) |
| `board_bookmarks` | `id` | Saved viewport positions |
| `task_runs` | `id` | Task-to-run junction (dispatch tracking) |
| `board_file_baselines` | `(project_id, file_path)` | File sync change detection |

**board_tasks:**
- `id` - Slug ID (e.g., "build-api")
- `project_id`, `parent_id`, `position`
- `name`, `status` (todo, doing, done, blocked)
- `content`, `x`, `y`

**board_evals:**
- `id` - Slug ID
- `project_id`, `name`, `status`
- `content`, `validates` (JSON array of task IDs)

---

## File Layout

```
~/.hirsel/
├── config.toml           # Initial config / one-time override (optional)
├── hirsel.db             # Global DB (config, credentials, projects, board, live_nodes)
├── key                   # Encryption key for credentials
├── hirsel.pid            # Daemon PID file
├── projects/{id}/        # Project-specific data
│   └── board/            # Agent file sync directory
│       ├── tasks/        # JSON files for each task
│       └── evals/        # JSON files for each eval
└── runs/{run_name}/
    ├── hirsel.db         # Run state (workers, messages, events, evals, history)
    ├── spec.md           # Specification (input)
    ├── eval.md           # Eval criteria (input)
    ├── assets/           # Images, files for spec/eval
    ├── work/             # Git worktrees
    │   ├── leader/
    │   └── worker-2/
    ├── chats/            # Generated from DB
    └── tmp/
        └── eval_log.md
```

**Note:** Work items (live_nodes) are stored in the global database, not per-run. This enables cross-run coordination and persistent project runs.

### Git Workspace Setup (`src-tauri/src/core/git.rs`, `ops/setup.rs`)

The git workspace structure differs based on whether the run uses single or multiple workers.

**Single-Worker Mode:**
```
work/
└── staging/              # Worker works directly here
    └── .git/             # Original project's .git (no remotes configured)
```
- The single worker uses the staging workspace directly
- No `origin` remote is configured (no push/pull needed)
- Changes stay local until run completion
- Worker instructions tell the agent: "No git push is needed"

**Multi-Worker Mode:**
```
work/
├── staging/              # Shared staging repository
│   └── .git/
│       └── config        # receive.denyCurrentBranch = updateInstead
├── leader/               # Worker clone (origin → staging/)
│   └── .git/
│       └── config        # origin = /path/to/staging
└── worker-2/             # Worker clone (origin → staging/)
    └── .git/
        └── config        # origin = /path/to/staging
```
- Each worker has an isolated clone with `origin` pointing to `staging/`
- Workers must `git push origin staging` before completing tasks
- `staging/` accepts pushes via `receive.denyCurrentBranch = updateInstead`
- Workers handle merge conflicts when pulling others' changes

This distinction is automatically detected in the worker prompt generation based on whether teammates are present.

---

## Configuration (`src-tauri/src/core/config/mod.rs`)

### DB-First Loading

Configuration is loaded with the following priority (later sources override earlier):

1. **Default values** - Built-in defaults
2. **Database** (`~/.hirsel/hirsel.db`) - Source of truth
3. **Config file** (`~/.hirsel/config.toml`) - Seeds DB on first run, or overrides DB
4. **Environment variables** - Always win

When a config file exists, its values are loaded into the database. This enables:
- Remote coordinators (Fly.io) to work without persistent volumes
- API-based config management via PUT/PATCH `/api/config`
- Config changes to persist across restarts

### Config Struct Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `root` | `PathBuf` | `~/.hirsel` | Hirsel root directory |
| `agent` | `AgentConfig` | - | Agent command configuration |
| `eval_timeout` | `u32` | `1800` | Eval timeout in seconds |
| `human_in_the_loop` | `bool` | `true` | HITL mode default |
| `runners` | `HashMap<String, RunnerConfig>` | `{}` | Named runner configs |
| `default_runner` | `Option<String>` | `None` | Default runner name |
| `profiles` | `HashMap<String, OrchestratorProfile>` | local | Orchestrator profiles |
| `storage` | `StorageConfig` | local | Storage backend config |
| `service_workers` | `ServiceWorkersConfig` | local | Service worker runner config |

### Runner Configuration (`src-tauri/src/core/runner/config.rs`)

```toml
# Local runner (default)
[runners.local]
host = "local"

# Local with Docker
[runners.local-docker]
host = "local"
[runners.local-docker.container]
image = "rust:latest"

# SSH runner
[runners.my-server]
[runners.my-server.host]
type = "ssh"
address = "user@server.com"
port = 22
work_base = "/tmp/hirsel"

# Fly runner
[runners.fly]
[runners.fly.host]
type = "fly"
app = "hirsel-workers"
region = "ams"
cpus = 2
memory_mb = 2048
[runners.fly.container]
image = "debian:bookworm-slim"
```

### Orchestrator Profiles

```toml
[profiles.local]
mode = "local"

[profiles.remote]
mode = "remote"
url = "https://hirsel-coordinator.fly.dev"
api_key = "secret"
default_runner = "fly"
```

### Service Workers Configuration

Service workers provide warm worker support for background services like Scribe.

```toml
[service_workers]
runner = "fly"                    # Default runner for service workers

[service_workers.scribe]
runner = "local"                  # Override for scribe (docs agent)
idle_timeout_seconds = 300        # 5 min default
```

**Resolution order:**
1. Service-specific runner (`service_workers.scribe.runner`)
2. Default service runner (`service_workers.runner`)
3. Local (fallback)

**Behavior:**
- `ScribeService.process_batch(run_name)` handles local vs remote internally
- If runner is "local" or unset: runs scribe directly in-process (no HTTP)
- If runner is a configured remote runner: spawns HTTP service worker, routes requests to it
- Callers don't need to know about local vs remote - just call `process_batch()`

---

## REST API (`src-tauri/src/core/server/mod.rs`)

**Auth:** `Authorization: Bearer $HIRSEL_API_KEY`

### Run Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/runs` | `list_runs` |
| POST | `/api/runs` | `create_run` |
| GET | `/api/runs/{name}` | `get_run` |
| DELETE | `/api/runs/{name}` | `delete_run` |
| GET | `/api/runs/{name}/files` | `download_files` |
| POST | `/api/runs/{name}/files` | `upload_files` |
| POST | `/api/runs/{name}/workspace` | `init_workspace` |
| POST | `/api/runs/{name}/spawn` | `spawn_workers` |
| POST | `/api/runs/{name}/pause` | `pause_run` |
| POST | `/api/runs/{name}/resume` | `resume_run` |
| POST | `/api/runs/{name}/deliver` | `deliver_run` |

### Worker/Message Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/runs/{name}/workers` | `list_workers` |
| POST | `/api/runs/{name}/workers/{w}/restart` | `restart_worker` |
| POST | `/api/runs/{name}/workers/{w}/spawn` | `spawn_single_worker` |
| POST | `/api/runs/{name}/workers/{w}/resume` | `resume_worker` |
| GET | `/api/runs/{name}/workers/{w}/events` | `get_worker_events` |
| POST | `/api/runs/{name}/scribe` | `add_scribe` |
| GET | `/api/runs/{name}/docs` | `get_docs` |
| POST | `/api/runs/{name}/docs/sync` | `sync_docs` |
| GET | `/api/runs/{name}/evals` | `list_evals` |
| GET | `/api/runs/{name}/history` | `get_history` |
| POST | `/api/runs/{name}/assets` | `upload_asset` |
| GET | `/api/runs/{name}/assets-path` | `get_assets_path` |
| GET | `/api/runs/{name}/threads` | `list_threads` |
| GET/POST | `/api/runs/{name}/threads/{t}/messages` | `get_messages`, `send_message` |

### Live Node Endpoints (via project context)

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/live-nodes` | `list_live_nodes` |
| POST | `/api/live-nodes` | `add_live_node` |
| POST | `/api/live-nodes/{id}/claim` | `claim_live_node` |
| POST | `/api/live-nodes/{id}/complete` | `complete_live_node` |
| POST | `/api/live-nodes/{id}/unclaim` | `unclaim_live_node` |

### Config Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/config` | `get_config` |
| PUT | `/api/config` | `put_config` - Replace config fields |
| PATCH | `/api/config` | `patch_config` - Merge partial config |
| PATCH | `/api/config/general` | `patch_general_config` |
| PATCH | `/api/config/agent` | `patch_agent_config` |
| GET/PUT/DELETE | `/api/config/runners/{name}` | runner CRUD |
| GET/PUT/DELETE | `/api/config/profiles/{name}` | profile CRUD |
| POST/GET/DELETE | `/api/credentials/{key}` | credential CRUD |

### Board Endpoints

| Method | Path | Handler |
|--------|------|---------|
| POST | `/api/board/{project_id}/export` | `export_board` - Export board to agent JSON files |
| POST | `/api/board/{project_id}/import` | `import_board` - Import board changes from agent JSON files |
| GET | `/api/board/{project_id}/directory` | `get_board_directory` - Get board directory path |
| GET | `/api/board/{project_id}/tasks` | `list_task_files` - List task files in board |
| GET | `/api/board/{project_id}/tasks/{slug}` | `get_task_file` - Get task file content |
| POST | `/api/board/{project_id}/tasks/{slug}` | `save_task_file` - Save task file |
| DELETE | `/api/board/{project_id}/tasks/{slug}` | `delete_task_file` - Delete task file |

### Gyp Chat Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/gyp/sessions` | `list_sessions` |
| POST | `/api/gyp/sessions` | `start_session` |
| DELETE | `/api/gyp/sessions/{id}` | `stop_session` |
| POST | `/api/gyp/sessions/{id}/messages` | `send_message` |
| POST | `/api/gyp/sessions/{id}/permission` | `respond_permission` |
| GET | `/api/gyp/sessions/{id}/events` | `session_events` (SSE) |

### Adding a REST Endpoint

Routes are shared between daemon and remote server via `shared_routes.rs` to avoid duplication:

| Builder | Used By | Description |
|---------|---------|-------------|
| `build_shared_routes()` | Both | Run ops, workers, tasks, messages, evals, history, assets |
| `build_gyp_routes()` | Both | Gyp chat sessions (requires `GypState`) |
| `build_config_routes()` | Remote only | Config CRUD, credentials |
| `build_board_routes()` | Remote only | Board sync for SpecFlow |
| Inline routes in `daemon/server.rs` | Daemon only | `/daemon/*`, worker internal API |

**To add a shared endpoint:**
1. Add handler in `routes.rs` (or appropriate module like `gyp.rs`, `board.rs`)
2. Add route to the appropriate builder in `shared_routes.rs`
3. Both servers automatically get the new route

**To add a daemon-only endpoint:**
Add the route inline in `daemon/server.rs` after the `build_shared_routes()` call.

**To add a remote-server-only endpoint:**
Add a new builder function or extend `build_config_routes()`/`build_board_routes()`.

---

## Daemon (`src-tauri/src/daemon/`)

Background process that owns lifecycle management.

**Listeners:**
- TCP: `0.0.0.0:{port}` where port is `HIRSEL_DAEMON_PORT` env var (default: 19700)
- CLI/GUI connects via localhost, Docker via host.docker.internal

**Port Configuration:**
- Set `HIRSEL_DAEMON_PORT` to use a different port (useful for testing or multiple instances)
- On startup, daemon checks if port is already in use and errors with helpful message
- Each `HIRSEL_ROOT` should use a unique port to avoid conflicts

**Polling Loop (every 5s):**
```rust
for run in active_runs {
    // Event-driven scaling: check if any task state changes requested scaling
    if state.consume_scaling_check()? {
        let actions = evaluate_scaling(&state)?;
        // Handle: SpawnWorker(assigned_task_id), WakeWorker(assigned_task_id)
        handle_lifecycle_actions(actions).await;
    }

    match status {
        Status::Working => {
            let actions = lifecycle.process_event(LifecycleEvent::TimeCheck)?;
            // Handle: EvalTriggered, RunFailed, TimeWarning
            handle_lifecycle_actions(actions).await;
            maybe_process_scribe(run);
        }
        Status::Eval => {
            // Check time limit
            // Detect eval process crash (PID no longer alive) → re-trigger eval
        }
    }
}
```

**Event-Driven Scaling:** Worker scaling is triggered by task state changes (add_task, complete_task, etc.) setting the `scaling_check_requested` DB flag. The daemon's 5-second poll provides natural debouncing - multiple rapid task changes get batched into one scaling evaluation. The `evaluate_scaling()` function uses tree-walk distance to assign nearby work tasks and distant eval tasks to workers.

**Eval Crash Detection:** When a run is in `Eval` state, the daemon checks if the eval process PID is still alive. If the process crashed, the daemon marks the eval as failed and re-triggers it by resetting the run to `Working` and processing a `TimeCheck` event.

**Auto-start:** CLI/GUI start daemon automatically via `DaemonOrchestrator::connect_or_start()`.

**Auto-exit:** Daemon exits after 5 minutes of no active runs.

---

## Design Notes

### Orchestrator Implementations

All orchestrator methods are fully implemented across Local, Daemon, and Remote:

| Implementation | Transport | Use Case |
|----------------|-----------|----------|
| `LocalOrchestrator` | Direct SQLite | Server, daemon internals |
| `DaemonOrchestrator` | TCP (HTTP) | CLI/GUI in local mode |
| `RemoteOrchestrator` | TCP (HTTP) | CLI/GUI in remote mode |

The daemon exposes the same HTTP API over TCP that the remote server exposes. This allows all orchestrator implementations to share the same route handlers.

### RemoteLifecycleManager (Intentional No-op)

`lifecycle/remote.rs` returns no-ops for all methods because remote workers delegate lifecycle management to the coordinator. The coordinator (running `LocalLifecycleManager`) handles:
- Eval triggering when workers go idle
- Worker scaling decisions
- Time limit enforcement

This is by design, not a stub that needs implementation.

### Partial/Feature-Gated Implementations

| Feature | Location | Status |
|---------|----------|--------|
| SSH runner | `runner/ssh.rs` | Works but less tested than Local/Fly |
| S3WorkspaceProvider | `draft/s3_workspace.rs` | Feature-gated (`s3-storage`) |
| S3SnapshotStrategy | `snapshot/s3.rs` | Feature-gated (`s3-storage`) |

### Runner Config Storage

Runner configurations are stored per-run at creation time in the `runner_configs` column (JSON). This ensures that changes to `config.toml` don't affect in-progress runs. The storage flow:

1. At run creation, resolve runner names to full `RunnerConfig` objects
2. Store the configs in the run's SQLite database
3. When spawning workers, use stored configs

Methods in `state/run.rs`:
- `get_runner_configs()` - Get stored configs
- `set_runner_configs()` - Store configs at run creation
- `get_runner_config_for_worker()` - Get config for a worker from stored configs

### Remote Worker Bootstrap

Ephemeral runners (Fly) bootstrap workers via init scripts in `runner/setup.rs`:

1. **Download hirsel binary** - From GitHub releases with version pinning (`HIRSEL_TAG=v{VERSION}`)
2. **Install dependencies** - Node.js for agent tools if not in image
3. **Fetch project files** - Tarball from coordinator's `/api/runs/{name}/files`
4. **Initialize git** - With coordinator as remote for syncing
5. **Start worker** - `hirsel __remote-worker` connects back to coordinator

The coordinator embeds its version at compile time and passes it to workers, ensuring binary compatibility.

### Service Workers

Service workers (`service_worker/`) manage background services that benefit from "warm" instances:

| Service | Purpose | Default Timeout |
|---------|---------|-----------------|
| Scribe | Documentation agent processing learnings | 5 min |
| ConflictResolver | AI-assisted git merge conflict resolution | 5 min |

**Architecture:**
```
ScribeService.process_batch(run_name)
    │
    ├─ should_use_remote()?
    │   │
    │   ├─ No → process_locally()
    │   │       └─ Run scribe directly (spawn_blocking + LocalSet)
    │   │
    │   └─ Yes → process_via_remote()
    │           │
    │           ├─ get_or_spawn_worker()
    │           │   ├─ Health check existing worker
    │           │   │   └─ Yes → reuse endpoint
    │           │   │   └─ No → spawn new worker
    │           │
    │           └─ POST to /scribe/batch endpoint
    │
    └─ Worker self-terminates after idle timeout
```

**Key files:**
- `core/service_worker/scribe.rs` - `ScribeService` handles local vs remote internally
- `core/service_worker/conflict_resolver.rs` - `ConflictResolverServiceWrapper` for merge conflicts
- `core/conflict_resolver/` - Core conflict resolution logic and ACP client
- `core/acp_runner.rs` - Shared ACP agent runner used by both services
- `core/service_worker/types.rs` - `ServiceWorkerType`, `ServiceWorkerHandle`
- `cli/service_worker.rs` - HTTP server for `__service-worker` command (remote only)
- `daemon/lifecycle.rs` - Integration in `maybe_process_scribe()`

**HTTP API (exposed by service worker binary):**
- `GET /health` - Health check with idle time
- `POST /scribe/batch` - Process scribe batch
- `POST /shutdown` - Graceful shutdown

### Docs Persistence

Project documentation is copied to the run directory at start, allowing Scribe to maintain it during the run without polluting the workspace git state.

**Flow:**
```
Run Start:
  workspace/docs/ ──copy──► run_dir/docs/
  git update-index --skip-worktree docs/*
  rm -rf workspace/docs/

During Run:
  Scribe writes to run_dir/docs/
  Workers read via read_docs MCP tool

Delivery (persist=true):
  run_dir/docs/ ──copy──► workspace/docs/
  git add docs/ && git commit

Delivery (persist=false):
  git checkout docs/  (restore original)
```

**Config:**
```toml
scribe_docs_path = "docs"           # Relative to workspace
scribe_persist_docs_changes = true  # Commit changes on delivery
```

**Key files:**
- `core/ops/docs.rs` - `setup_docs()`, `deliver_docs()`
- `core/scribe.rs` - Scribe agent prompt and batch processing

### Known Constraints

- **Fly requires container.image** - Machines ARE containers
- **Client host only in remote mode** - Requires Tailscale for SSH-back
- **SSH reverse tunnel required for local mode** - Workers connect to `localhost:19700`
- **Worker binary version** - Must match coordinator version (auto-pinned via `HIRSEL_TAG`)

### Gotchas

#### Tauri Commands Use camelCase Parameters

Tauri automatically converts Rust snake_case parameters to JavaScript camelCase. Use camelCase in the frontend:

```rust
// Backend (Rust) - uses snake_case
#[tauri::command]
pub async fn delete_board_task(project_id: i64, task_id: String) -> Result<(), String> {
    // ...
}
```

```typescript
// Frontend (TypeScript) - CORRECT: use camelCase
await invoke('delete_board_task', { projectId, taskId });

// Frontend (TypeScript) - WRONG: snake_case won't match
await invoke('delete_board_task', { project_id: projectId, task_id: taskId });
```

If a command errors with "missing required key", check that you're using camelCase parameter names.

---

## Testing

### E2E Tests (`tests/e2e/`)

Pytest-based tests with runner × scenario matrix.

```
tests/e2e/
├── pyproject.toml          # uv project config
├── conftest.py             # pytest fixtures, parametrization
├── test_runs.py            # Main tests (3 classes)
├── orchestrator.py         # Orchestrator setup (LOCAL, REMOTE)
└── runners/                # Runner implementations
    ├── base.py             # BaseRunner, RunnerConfig
    ├── local.py            # LocalRunner
    ├── docker.py           # DockerRunner
    ├── ssh.py              # SshRunner
    └── fly.py              # FlyRunner
```

**Test classes:**
- `TestRunScenario` - Main parametrized runner × scenario tests
- `TestPauseResume` - Pause/resume functionality (`@pytest.mark.slow`)
- `TestDockerLifecycle` - Docker container cleanup (`@pytest.mark.docker`)

**Running tests:**
```bash
cd tests/e2e
uv sync
uv run pytest                                    # Default (local, noop)
uv run pytest --runner=fly --scenario=calculator # Specific
uv run pytest -m "not slow"                      # Skip slow
uv run pytest --profile=fly                      # Remote orchestrator
```

**Adding a runner:**
1. Create `runners/myrunner.py` extending `BaseRunner`
2. Implement `configure()`, `verify_output()`, `skip_if_unavailable()`
3. Register in `conftest.py` → `RUNNERS` dict

### Unit Tests

Inline Rust tests in source files (`#[test]`, `#[tokio::test]`).
- CLI modules have unit tests for argument parsing
- Board module has tests for task/eval operations

Run with:
```bash
cargo nextest run
```

### MCP UI Tests (`tests/e2e/create-run.test.ts`)

TypeScript test for UI automation via MCP client.

### Scenarios (`tests/scenarios/`)

| Scenario | Purpose |
|----------|---------|
| `hello_world` | Basic file creation |
| `calculator` | Python with pytest |
| `noop` | Minimal validation |
| `todo_api` | FastAPI with eval |
| `multi_file` | Multi-file changes |
| `tic_tac_toe` | Game implementation |
