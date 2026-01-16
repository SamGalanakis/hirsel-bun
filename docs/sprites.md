# Sprites Runner Implementation

## Overview

Sprites.dev provides cloud VMs (Firecracker-based) with built-in networking features that enable a "batteries included" remote worker experience without requiring users to set up Tailscale or tunnels.

## Key Sprites Features

| Feature | Description |
|---------|-------------|
| **Public URL** | Every sprite gets `https://<name>.sprites.app` routing to port 8080 |
| **Exec API** | Run commands via HTTP API (no SSH needed) |
| **Port Proxy** | WebSocket tunnel to any port inside sprite |
| **Sessions** | Detachable TTY sessions for long-running processes |
| **Checkpoints** | Snapshot and restore VM state |

## The Connectivity Problem

```
Local runner:    Worker → localhost:19700 ✅ (same machine)
SSH runner:      Worker → tunnel → Coordinator ✅ (reverse SSH tunnel)
Sprite runner:   Worker → ??? → Coordinator ❌ (no reverse tunnel available)
```

Sprites can't reach a coordinator running on `localhost`. Unlike SSH, there's no reverse tunnel capability.

**However**, Sprites DO have inbound connectivity:
- Public URL: `https://<sprite-name>.sprites.app`
- Coordinator CAN reach the sprite

## Solution: Push Model for Remote Workers

Instead of workers pulling from coordinator, coordinator pushes to workers.

### Architecture Comparison

```
Pull Model (Local):
  Worker ──HTTP──► Coordinator (worker initiates)

Push Model (Remote):
  Coordinator ──HTTP──► Worker (coordinator initiates)
```

### Unified Remote Worker Design

All remote workers (SSH and Sprite) use the same push-based model:

| Runner | Transport | Worker Role |
|--------|-----------|-------------|
| Local | Direct | Client (pulls) |
| SSH | Reverse tunnel | Server (receives pushes) |
| Sprite | Public URL | Server (receives pushes) |

**Worker code is identical for SSH and Sprite** - the runner handles transport differences.

## Worker HTTP Server

Remote workers run an HTTP server instead of being HTTP clients:

```rust
// Remote worker serves these endpoints
let app = Router::new()
    // Task management
    .route("/tasks", post(receive_tasks))       // Coordinator pushes task updates
    .route("/status", get(report_status))       // Coordinator queries status

    // Communication
    .route("/messages", post(receive_message))  // Coordinator pushes messages

    // Control
    .route("/pause", post(handle_pause))
    .route("/resume", post(handle_resume))
    .route("/shutdown", post(handle_shutdown))

    // Output (for attach)
    .route("/heartbeat", post(heartbeat))       // Includes recent output lines

    // Git (coordinator does git operations TO worker)
    .nest("/git", git_http_router(repo_path));  // Reuse existing git_http.rs
```

## Git Sync in Push Model

**Current (pull model):** Worker does `git pull/push` to coordinator's git server.

**Push model:** Coordinator does `git pull/push` to worker's git server.

```
Pull Model:
  Worker A ──git push──► Coordinator ◄──git push── Worker B

Push Model:
  Coordinator ──git push/pull──► Worker A
  Coordinator ──git push/pull──► Worker B
  (Coordinator syncs between workers)
```

Worker runs the same `git_http.rs` server that coordinator currently runs. Coordinator becomes the git client.

## Runner Trait Extensions

```rust
pub struct SpawnResult {
    pub handle: WorkerHandle,
    pub pid: Option<u32>,
    pub worker_url: Option<String>,  // NEW: for push-based runners
}

trait Runner {
    // ... existing methods ...

    /// Does this runner use push model?
    fn is_push_based(&self) -> bool { false }

    /// Stream logs from worker (for attach)
    async fn stream_logs(&self, handle: &WorkerHandle) -> RunnerResult<LogStream>;
}
```

## Coordinator Changes

```rust
impl Coordinator {
    async fn notify_worker(&self, worker: &WorkerHandle, event: Event) {
        if self.runner.is_push_based() {
            // Push to worker URL
            self.push_to_worker(worker, event).await;
        } else {
            // Worker will pull (current behavior)
        }
    }

    async fn sync_git(&self) {
        for worker in &self.workers {
            if self.runner.is_push_based() {
                // Pull changes from worker
                git_pull(&format!("{}/git", worker.url)).await;
            }
        }
        // Push combined changes back
        for worker in &self.workers {
            if self.runner.is_push_based() {
                git_push(&format!("{}/git", worker.url)).await;
            }
        }
    }
}
```

## Feature Parity

| Feature | Local | SSH | Sprite |
|---------|-------|-----|--------|
| Task sync | Pull | Push | Push |
| Git sync | Pull | Push | Push |
| Messages | Pull | Push | Push |
| Attach | DB stream | DB stream | DB stream |
| Pause/Resume | Direct | Push | Push |
| Logs | File | Exec API | Exec API |

**Attach works unchanged** - UI streams from coordinator's DB via `state.get_worker_events()`.

### How Attach Works (Current)

```
Worker writes events → SQLite DB → Backend polls (200ms) → Tauri emit → UI
```

The GUI always reads from the **coordinator's database**. For remote workers, events need to reach that DB somehow:

| Runner | How events reach coordinator DB |
|--------|--------------------------------|
| Local | Worker writes directly (same DB) |
| SSH (current) | Worker writes via HTTP API through tunnel |
| Push model | Worker pushes events in heartbeat → coordinator writes |

### Push Model Heartbeat

```rust
// Worker sends heartbeat every ~500ms when active
POST /heartbeat
{
    "status": "working",
    "current_task": "implement_auth",
    "events_since": 12345,  // Last event ID coordinator has
    "events": [
        { "id": 12346, "type": "text", "content": "Let me..." },
        { "id": 12347, "type": "tool_start", "tool_title": "Read", ... }
    ]
}

// Coordinator writes events to its DB
// GUI polls DB via existing get_worker_events() - no changes needed
```

This keeps the attach implementation completely unchanged - only the transport differs.

## Sprites-Specific Implementation

### Creating a Sprite Worker

```rust
impl Runner for SpriteRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        // 1. Create sprite via API
        let sprite = self.client.create(&sprite_name).await?;

        // 2. Setup: clone repo, install deps
        self.client.exec(&sprite_name, setup_commands).await?;

        // 3. Start worker HTTP server (detached)
        self.client.exec_detached(&sprite_name, &[
            "hirsel", "__remote-worker-server",
            "--port", "8080",
            "--work-dir", "/home/sprite/work"
        ]).await?;

        // 4. Return handle with public URL
        Ok(SpawnResult {
            handle: WorkerHandle { ... },
            pid: None,
            worker_url: Some(format!("https://{}.sprites.app", sprite_name)),
        })
    }

    fn is_push_based(&self) -> bool { true }

    async fn stream_logs(&self, handle: &WorkerHandle) -> RunnerResult<LogStream> {
        // Use Sprites exec API to tail logs
        self.client.exec_stream(&handle.runner_id, &["tail", "-f", "/tmp/worker.log"]).await
    }
}
```

### Authentication

Sprite URLs are private by default (require auth token). Coordinator uses the same Sprites API token for:
- Creating/managing sprites
- Pushing to worker endpoints

## Comparison: Tailscale vs Push Model

| Aspect | Tailscale | Push Model |
|--------|-----------|------------|
| User setup | Install Tailscale, join network | None |
| Worker code | Same as local (pull) | Different (server) |
| Architecture | Uniform pull | Local=pull, Remote=push |
| Dependencies | External (Tailscale) | None |
| Complexity | User-facing | Implementation |

**Trade-off:** More implementation work, but zero user setup for Sprites.

## Implementation Plan

1. **Worker server mode** - Add HTTP server to remote worker
2. **Runner trait extension** - Add `is_push_based()`, `worker_url`
3. **Coordinator push logic** - Handle push vs pull based on runner
4. **Git flip** - Coordinator as git client for push runners
5. **Sprite runner update** - Use public URL, remove broken localhost code
6. **SSH runner update** - Also use push model for consistency

## Open Questions

1. **Heartbeat interval** - How often should workers push status/output?
2. **Git sync frequency** - How often should coordinator sync between workers?
3. **Error handling** - What if coordinator can't reach worker temporarily?
4. **Auth** - Should worker verify coordinator's identity? (Currently trusts Sprites auth)
