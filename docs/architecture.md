# Hirsel Architecture Reference

> **Keep this document updated** when modifying core modules.

## Quick Reference

### Common Modification Points

| Task | Files to Modify |
|------|-----------------|
| Add CLI command | `src/cli/mod.rs:95` (Commands enum), new `src/cli/<cmd>.rs` |
| Add GUI command | `src/gui/commands/mod.rs:36` (get_handlers), new handler in relevant submodule |
| Add REST endpoint | `src/core/server/mod.rs:71` (router), `src/core/server/routes.rs` |
| Modify run state | `src/core/state/mod.rs:27` (SCHEMA), `src/core/state/types.rs` |
| Add runner type | `src/core/runner/mod.rs:57`, new `src/core/runner/<type>.rs` |
| Modify lifecycle | `src/core/lifecycle/mod.rs`, `src/core/lifecycle/local.rs` |
| Add archive strategy | `src/core/snapshot/mod.rs`, new strategy impl of `ArchiveStrategy` |

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
    (direct access)     (Unix socket)        (HTTP API)
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│   SQLite State              │   Worker Processes                 │
│   (runs, workers, tasks,    │   (Local, SSH, Sprite, Fly)        │
│    messages, events)        │   + optional Docker container      │
└─────────────────────────────┴───────────────────────────────────┘
```

---

## Module Map

### `src/core/` - Core Business Logic

| Submodule | Key Files | Purpose |
|-----------|-----------|---------|
| `state/` | `mod.rs`, `types.rs`, `run.rs`, `workers.rs`, `tasks.rs`, `messages.rs`, `events.rs`, `evals.rs`, `history.rs` | SQLite state management |
| `orchestrator/` | `mod.rs:229`, `local.rs`, `remote.rs`, `daemon.rs` | Run orchestration pattern |
| `lifecycle/` | `mod.rs:181`, `local.rs`, `remote.rs`, `transitions.rs` | Event-driven state machine |
| `runner/` | `types.rs:139`, `local.rs`, `fly.rs`, `sprite.rs`, `ssh.rs`, `composed.rs`, `config.rs`, `setup.rs` | Worker host implementations |
| `snapshot/` | `mod.rs`, `archive.rs`, `noop.rs`, `s3.rs`, `sprite_checkpoint.rs`, `claude_session.rs` | Work/session persistence |
| `draft/` | `mod.rs`, `types.rs:11`, `workspace.rs`, `local_workspace.rs`, `s3_workspace.rs` | StartingPoint, workspace init |
| `config/` | `mod.rs:144`, `types.rs`, `agent.rs`, `storage.rs`, `orchestrator.rs`, `paths.rs` | Config struct, profiles, runners |
| `ops/` | `mod.rs`, `run.rs`, `setup.rs`, `spawn.rs`, `project.rs`, `types.rs` | Shared CLI/GUI operations |
| `server/` | `mod.rs:32`, `routes.rs`, `auth.rs`, `gyp.rs` | HTTP server for remote mode |
| `files.rs` | - | Run directory file operations |
| `chats.rs` | - | GypChat message storage |
| `gyp_chat.rs` | - | Project-level chat history |
| `acp.rs` | - | Agent Control Protocol types |
| `api_types.rs` | - | Shared API response types |
| `worker_routes.rs` | - | Worker HTTP handlers |
| `credentials.rs` | - | Encrypted credential store |
| `git.rs` | - | Git operations |
| `compaction.rs` | - | Context compaction for long sessions |

### `src/worker/` - Worker Subprocess

| File | Purpose |
|------|---------|
| `acp_client.rs` | ACP connection, message handling, prompt building |
| `runner.rs` | Worker execution loop (`WorkerRunner`) |
| `msg.rs` | Message types and serialization |
| `mcp.rs` | MCP server for worker tools |
| `eval_mcp.rs` | MCP server for eval tools |
| `remote_runner.rs` | Remote worker entry point |
| `http_state.rs` | HTTP-based state for remote workers |
| `file_server.rs` | File upload server for remote workers |

### `src/gui/commands/` - Tauri IPC Commands

| File | Commands |
|------|----------|
| `runs.rs` | `get_runs`, `get_run_detail`, `pause_run`, `resume_run`, `delete_run`, `deliver_run` |
| `drafts.rs` | `validate_repo`, `create_draft`, `clone_run`, `update_draft`, `start_draft` |
| `workers.rs` | `get_workers`, `attach_worker`, `open_worker_terminal`, `restart_worker` |
| `tasks.rs` | `get_tasks`, `add_task`, `delete_task`, `complete_task`, `reopen_task` |
| `messages.rs` | `get_messages`, `get_threads`, `send_message`, `mark_messages_read` |
| `events.rs` | `get_worker_events`, `start_worker_event_stream`, `stop_worker_event_stream` |
| `chat.rs` | `start_chat_session`, `send_chat_message`, `respond_chat_permission` |
| `config_cmd.rs` | `get_config`, `save_config`, `get_tailscale_info` |
| `credentials.rs` | `store_credential`, `delete_credential`, `has_credential` |
| `files.rs` | `read_spec_file`, `write_spec_file`, `save_asset` |
| `filesystem.rs` | `pick_folder`, `suggest_paths` |
| `logs.rs` | `get_eval_log`, `get_history`, `get_evals` |

### `src/cli/` - CLI Commands

| File | Command | Feature |
|------|---------|---------|
| `go.rs` | `hirsel go <run> <spec>` | `cli` |
| `runs.rs` | `hirsel runs` | - |
| `view.rs` | `hirsel view <run>` | - |
| `attach.rs` | `hirsel attach <run>` | `cli` |
| `pause.rs` | `hirsel pause <run>` | - |
| `resume.rs` | `hirsel resume <run>` | - |
| `delete.rs` | `hirsel delete <run>` | - |
| `deliver.rs` | `hirsel deliver <run>` | - |
| `msg.rs` | `hirsel msg <run>` | - |
| `tasks.rs` | `hirsel tasks <run>` | - |
| `diff.rs` | `hirsel diff <run>` | - |
| `summary.rs` | `hirsel summary <run>` | - |
| `config.rs` | `hirsel config` | - |
| `test.rs` | `hirsel test <scenario>` | `cli` |
| `acp_bridge.rs` | `hirsel __acp-bridge` | - |

### `src/daemon/` - Background Process

| File | Purpose |
|------|---------|
| `mod.rs` | Socket/PID paths, `is_daemon_running()` |
| `server.rs` | Daemon server, TCP + Unix socket listeners |
| `lifecycle.rs` | Polling loop, lifecycle action handling |
| `client.rs` | Client for daemon communication |

---

## Key Traits

### `Orchestrator` (`src/core/orchestrator/mod.rs:229`)

High-level run management interface. CLI, GUI, and server use this trait.

```rust
pub trait Orchestrator: Send + Sync {
    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>>;
    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail>;
    async fn delete_run(&self, name: &str) -> OrchestratorResult<()>;
    async fn pause_run(&self, name: &str) -> OrchestratorResult<()>;
    async fn resume_run(&self, name: &str, time_limit: Option<u32>) -> OrchestratorResult<()>;
    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>>;
    async fn list_tasks(&self, run: &str) -> OrchestratorResult<Vec<Task>>;
    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail>;
    async fn init_workspace(&self, run: &str, request: InitWorkspaceRequest) -> OrchestratorResult<InitWorkspaceResponse>;
    async fn spawn_single_worker(&self, run: &str, worker: &str, work_dir: &Path, session_id: Option<&str>) -> OrchestratorResult<()>;
    async fn resume_worker(&self, run: &str, worker: &str, work_dir: &Path, session_id: Option<&str>, state: Option<&WorkerStateHandle>) -> OrchestratorResult<()>;
    // ... more methods
}
```

| Implementation | Location | Use Case |
|----------------|----------|----------|
| `LocalOrchestrator` | `orchestrator/local.rs` | Direct SQLite access (daemon, server) |
| `DaemonOrchestrator` | `orchestrator/daemon.rs` | Unix socket/TCP to daemon (CLI/GUI local mode) |
| `RemoteOrchestrator` | `orchestrator/remote.rs` | HTTP API to remote server |

### `LifecycleManager` (`src/core/lifecycle/mod.rs:181`)

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
| `TimeCheck` | `SpawnWorker`, `ResumeWorker`, `EvalTriggered`, `RunFailed`, `TimeWarning` |
| `WorkerDone` | `EvalTriggered`, `RunCompleted` |
| `PauseRequested` | `WorkersPaused`, `RunStatusChanged` |
| `ResumeRequested` | `ResumeWorker` |

| Implementation | Location | Use Case |
|----------------|----------|----------|
| `LocalLifecycleManager` | `lifecycle/local.rs` | Local/daemon mode |
| `RemoteLifecycleManager` | `lifecycle/remote.rs` | Remote workers (delegates to coordinator) |

### `Runner` (`src/core/runner/types.rs:139`)

Worker spawning interface. Host + optional Container model.

```rust
#[async_trait]
pub trait Runner: Send + Sync {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult>;
    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()>;
    async fn is_alive(&self, handle: &WorkerHandle) -> bool;
    fn runner_type(&self) -> &'static str;
    fn is_ephemeral(&self) -> bool; // true for Fly, Sprite
}
```

| Implementation | Location | Host Type | Ephemeral |
|----------------|----------|-----------|-----------|
| `LocalRunner` | `runner/local.rs` | Local machine | No |
| `SshRunner` | `runner/ssh.rs` | Remote via SSH | No |
| `SpriteRunner` | `runner/sprite.rs` | Sprites.dev VM | Yes |
| `FlyRunner` | `runner/fly.rs` | Fly.io machine | Yes |
| `ComposedRunner` | `runner/composed.rs` | Executor + Resource | Varies |

### `ArchiveStrategy` (`src/core/snapshot/archive.rs`)

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
| `SpriteCheckpointStrategy` | `snapshot/sprite_checkpoint.rs` | Sprite | Sprites API |

### `WorkspaceProvider` (`src/core/draft/workspace.rs`)

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

## State Machine

### Run Status (`src/core/state/types.rs:15`)

```
Draft ──start──► Working ──eval──► Eval ──pass──► Done ──deliver──► Delivered
                    │                  │
                    │                  ▼
                  pause            fail (max retries)
                    │                  │
                    ▼                  ▼
                 Paused              Failed
                    │
                  resume
                    │
                    ▼
                 Working
```

| Status | Description | Terminal |
|--------|-------------|----------|
| `Draft` | Configured, workers not spawned | No |
| `Working` | Workers actively running | No |
| `Paused` | Manually paused by user | No |
| `Eval` | Evaluation in progress | No |
| `Done` | Completed successfully | Yes |
| `Delivered` | Changes pushed to branch | Yes |
| `Failed` | Run failed (see `failure_reason`) | Yes |

### Worker Status (`src/core/state/types.rs:151`)

| Status | Description |
|--------|-------------|
| `Working` | Actively processing |
| `Awaiting` | Idle (no work or waiting for user) |
| `Paused` | Stopped (run is paused) |
| `Error` | Process died unexpectedly |

### Task Status (`src/core/state/types.rs:201`)

| Status | Description |
|--------|-------------|
| `Todo` | Not started |
| `Doing` | Claimed by worker |
| `Done` | Completed |

---

## Data Flow

### Run Creation (Local Mode)

```
CLI/GUI
   │
   ▼ StartRunRequest
DaemonOrchestrator ──Unix socket──► Daemon
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

### Worker Lifecycle

```
Daemon ──spawn──► Worker Process
                      │
                      ▼
               AcpClient.connect()
                      │
                      ▼
               Send initial prompt (spec + tasks)
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
                      ▼ (no more work)
              worker_done()
                      │
                      ▼
              Daemon lifecycle poll
                      │
                      ▼
              Trigger eval (if all workers idle)
```

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
   └─► Returns ResumeWorker actions
         │
         ▼
Daemon handles each ResumeWorker:
   1. Check if runner is_ephemeral()
   2. If yes: ArchiveStrategy.restore(work_dir_handle)
   3. ArchiveStrategy.restore(session_handle)
   4. Runner.spawn() with resume_session_id
   5. Update worker DB (clear state handle, set status)
```

---

## Database Schema (`src/core/state/mod.rs:27`)

### Tables

| Table | Primary Key | Purpose |
|-------|-------------|---------|
| `state` | `id=1` | Run metadata (singleton) |
| `workers` | `id` | Worker processes |
| `tasks` | `id` (text) | Work items |
| `messages` | `id` | Chat threads |
| `message_reads` | `(worker_name, thread)` | Read tracking |
| `worker_events` | `id` | Real-time output streaming |
| `evals` | `id` | Evaluation runs |
| `history` | `id` | Activity log |
| `amendments` | `id` | Spec amendments |

### Key Columns

**state:**
- `status`, `failure_reason`, `started_at`, `time_limit_minutes`
- `worker_scale`, `max_iterations`, `human_in_the_loop`
- `default_runner`, `worker_runners` (JSON), `runner_configs` (JSON), `starting_point` (JSON)

**workers:**
- `name`, `pid`, `runner_id`, `runner_type`, `status`
- `session_id`, `work_dir`, `hitl_waiting`
- `state_handle` (JSON: WorkerStateHandle with work_dir and agent_session snapshots)

**tasks:**
- `id`, `name`, `status`, `claimed_by`, `claimed_at`
- `parent_id`, `blocked_by`

---

## File Layout

```
~/.hirsel/
├── config.toml           # Global configuration
├── hirsel.db             # Global DB (credentials)
├── hirsel.pid            # Daemon PID file
└── runs/{run_name}/
    ├── hirsel.db         # Run state (SOURCE OF TRUTH)
    ├── spec.md           # Specification (input)
    ├── eval.md           # Eval criteria (input)
    ├── tasks.md          # Generated from DB
    ├── tasks/            # Task detail files
    ├── assets/           # Images, files for spec/eval
    ├── work/             # Git worktrees
    │   ├── leader/
    │   └── worker-2/
    ├── chats/            # Generated from DB
    └── tmp/
        └── eval_log.md
```

---

## Configuration (`src/core/config/mod.rs:144`)

### Config Struct Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `root` | `PathBuf` | `~/.hirsel` | Hirsel root directory |
| `agent` | `AgentConfig` | - | Agent command configuration |
| `eval_timeout` | `u32` | `1800` | Eval timeout in seconds |
| `human_in_the_loop` | `bool` | `true` | HITL mode default |
| `compaction_enabled` | `bool` | `true` | Enable context compaction |
| `compaction_threshold` | `Option<u32>` | `10000` | Token threshold |
| `runners` | `HashMap<String, RunnerConfig>` | `{}` | Named runner configs |
| `default_runner` | `Option<String>` | `None` | Default runner name |
| `profiles` | `HashMap<String, OrchestratorProfile>` | local | Orchestrator profiles |
| `storage` | `StorageConfig` | local | Storage backend config |

### Runner Configuration (`src/core/runner/config.rs`)

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

# Sprite runner
[runners.sprite]
[runners.sprite.host]
type = "sprite"
api_token = "..."
checkpoint = "hirsel-v1"
auto_destroy = true

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

---

## REST API (`src/core/server/mod.rs:71`)

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

### Worker/Task/Message Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/runs/{name}/workers` | `list_workers` |
| POST | `/api/runs/{name}/workers/{w}/restart` | `restart_worker` |
| POST | `/api/runs/{name}/workers/{w}/spawn` | `spawn_single_worker` |
| POST | `/api/runs/{name}/workers/{w}/resume` | `resume_worker` |
| GET | `/api/runs/{name}/workers/{w}/events` | `get_worker_events` |
| GET/POST | `/api/runs/{name}/tasks` | `list_tasks`, `add_task` |
| DELETE | `/api/runs/{name}/tasks/{id}` | `delete_task` |
| GET | `/api/runs/{name}/threads` | `list_threads` |
| GET/POST | `/api/runs/{name}/threads/{t}/messages` | `get_messages`, `send_message` |

### Config Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/config` | `get_config` |
| PATCH | `/api/config/general` | `patch_general_config` |
| PATCH | `/api/config/agent` | `patch_agent_config` |
| GET/PUT/DELETE | `/api/config/runners/{name}` | runner CRUD |
| GET/PUT/DELETE | `/api/config/profiles/{name}` | profile CRUD |
| POST/GET/DELETE | `/api/credentials/{key}` | credential CRUD |

---

## Daemon (`src/daemon/`)

Background process that owns lifecycle management.

**Listeners:**
- TCP: `0.0.0.0:19700` (CLI/GUI via localhost, Docker via host.docker.internal)

**Polling Loop (every 5s):**
```rust
for run in active_runs {
    let actions = lifecycle.process_event(LifecycleEvent::TimeCheck)?;
    for action in actions {
        match action {
            SpawnWorker { worker_name, work_dir } => {
                orchestrator.spawn_single_worker(...).await?;
            }
            ResumeWorker { worker_name, work_dir, session_id, snapshot, agent_session } => {
                orchestrator.resume_worker(...).await?;
            }
            EvalTriggered => { /* eval spawned by lifecycle */ }
            RunFailed { reason } => { /* update state */ }
            TimeWarning { percent } => { /* send notification */ }
            // ...
        }
    }
}
```

**Auto-start:** CLI/GUI start daemon automatically via `DaemonOrchestrator::connect_or_start()`.

**Auto-exit:** Daemon exits after 5 minutes of no active runs.

---

## Design Notes

### Orchestrator Implementations

All orchestrator methods are fully implemented across Local, Daemon, and Remote:

| Implementation | Transport | Use Case |
|----------------|-----------|----------|
| `LocalOrchestrator` | Direct SQLite | Server, daemon internals |
| `DaemonOrchestrator` | Unix socket (HTTP) | CLI/GUI in local mode |
| `RemoteOrchestrator` | TCP (HTTP) | CLI/GUI in remote mode |

The daemon exposes the same HTTP API over Unix socket that the remote server exposes over TCP. This allows all orchestrator implementations to share the same route handlers.

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
3. When spawning workers, use stored configs with fallback to global config (for backwards compatibility)

Methods in `state/run.rs`:
- `get_runner_configs()` - Get stored configs
- `set_runner_configs()` - Store configs at run creation
- `get_runner_config_for_worker()` - Get config for a worker (stored → global fallback)

### Remote Worker Bootstrap

Ephemeral runners (Fly, Sprite) bootstrap workers via init scripts in `runner/setup.rs`:

1. **Download hirsel binary** - From GitHub releases with version pinning (`HIRSEL_TAG=v{VERSION}`)
2. **Install dependencies** - Node.js for agent tools if not in image
3. **Fetch project files** - Tarball from coordinator's `/api/runs/{name}/files`
4. **Initialize git** - With coordinator as remote for syncing
5. **Start worker** - `hirsel __remote-worker` connects back to coordinator

The coordinator embeds its version at compile time and passes it to workers, ensuring binary compatibility.

### Known Constraints

- **Sprites cannot run Docker** - Firecracker VMs don't support nested containers
- **Fly requires container.image** - Machines ARE containers
- **Client host only in remote mode** - Requires Tailscale for SSH-back
- **SSH reverse tunnel required for local mode** - Workers connect to `localhost:19700`
- **Worker binary version** - Must match coordinator version (auto-pinned via `HIRSEL_TAG`)
