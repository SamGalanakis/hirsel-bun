# Hirsel

AI-powered software engineering tool. Users define specs on a visual board (SpecFlow), dispatch work to AI agent workers, monitor progress, and deliver changes via git.

**Stack:** Rust/Tauri backend, SolidJS/Tailwind v4 frontend, SQLite state, daemon process model.

## Rules
- Never leave legacy code paths, TODO/FIXME comments, or deprecated code
- Never duplicate logic across CLI/GUI — use shared abstractions in `core/ops/`
- Finish refactors completely — no half measures
- No backwards compatibility needed — modify schema directly
- Read `docs/architecture.md` before implementing; update it when adding modules/traits/APIs

## Docs
- `docs/architecture.md` — module map, traits, data flow, schema, API, where to add code (**read first**)
- `docs/debugging.md` — logs, profiling, reset commands, debug panel
- `docs/fly-deployment.md` — remote Fly.io runner deployment
- `docs/basecoatui.md` — Basecoat component patterns (shadcn/ui for vanilla HTML)
- `docs/design-preview.html` — visual design language (open in browser)
- `docs/future.md` — planned: gix migration, custom font

## Architecture at a Glance
- **Daemon** (`hirsel __daemon`) — background singleton managing run lifecycle, worker scaling, eval triggering
  - Polls every 5s, event-driven scaling via DB flag, auto-exits after 5min idle
  - PID file: `~/.hirsel/hirsel.pid`, default port 19700 (`HIRSEL_DAEMON_PORT` to override)
- **Orchestrator trait** — `Local` (direct SQLite), `Daemon` (TCP), `Remote` (HTTP API)
- **LifecycleManager trait** — event-driven state machine returning actions for daemon
- **Runner trait** — `Local`, `SSH`, `Fly`, `Composed` worker host implementations
- **Workers** — spawned with pre-assigned tasks, fresh context per task, no claim_task tool
- **SpecFlow** — visual board → dispatch → live nodes → workers → eval → delivery
- **Routes** — parallel exploration branches within a project (independent trees/docs/messages)
- **Delta dispatch** — draft tree → live tree diffs, persistent project runs
- **Service workers** — warm background agents (Scribe for docs, ConflictResolver for merges)

## Run Lifecycle
- `Draft` → `Working` → `Eval` → `Done` → `Delivered`
- Also: `Paused` (manual), `Failed` (time limit/error)
- Workers: `Working` → `Awaiting` → `Paused` → `Error`
- Live nodes: `Pending` → `Working` → `Done`/`Failed`

## Frontend
- **SolidJS** — components in `src/components/`, stores in `src/stores/`, hooks in `src/hooks/`
- **Stores:** AppProvider, ProjectProvider, RunsProvider, SelectionProvider, DeltaProvider, RouteProvider
- **Icons:** `<Icon name="icon-name" class="w-4 h-4" />` from `src/components/shared`
- **Toast:** `window.toast.success('msg')` / `window.toast.error('msg')`
- **Basecoat UI** — shadcn/ui patterns, see `docs/basecoatui.md`
- **Lucide Icons** — https://lucide.dev/icons/
- **Graph layout** — ELK.js in `src/lib/elk-layout.ts` for SpecBoard tree rendering
- **Tauri invoke** — params are camelCase in TS (`projectId`), snake_case in Rust (`project_id`)

## Backend
- Core logic: `src-tauri/src/core/` (state, board, delta, runner, lifecycle, orchestrator, etc.)
- CLI: `src-tauri/src/cli/` — `runs`, `view`, `attach`, `tasks`, `summary`, `deliver`, etc.
- GUI commands: `src-tauri/src/gui/commands/` — Tauri IPC handlers
- Daemon: `src-tauri/src/daemon/` — server, lifecycle polling, client
- Worker: `src-tauri/src/worker/` — ACP client, MCP tools, execution loop
- Process management: `AcpChild` in `core/acp.rs` — SIGTERM → wait → SIGKILL cleanup

## Data Locations
- `~/.hirsel/` — config, global DB (`hirsel.db`), runs, projects, PID file
- `~/.hirsel/runs/{name}/` — per-run SQLite, spec.md, eval.md, work dirs
- `~/.hirsel/projects/{id}/routes/{name}/` — route-scoped docs, board tasks, code
- `~/.local/share/app.hirsel/` — Tauri logs
- **Reset:** `rm -rf ~/.hirsel/runs` (runs) or `rm -rf ~/.hirsel ~/.local/share/app.hirsel` (full)

## Dev Commands
```bash
./dev.sh                    # Run dev build
./dev.sh --profiling        # With backend+frontend profiling
cargo build                 # Build to src-tauri/target/debug/hirsel
bun run lint                # Biome lint
bun run lint:fix            # Biome lint + fix
bun run test                # cargo nextest
typos                       # Spell check
prek                        # Pre-commit hooks on staged files
```

## Cargo Features
- `gui` (default) — Tauri desktop, includes `cli`
- `cli` — full CLI with server + TUI attach
- `server` — HTTP server, daemon
- `worker` — minimal remote worker binary
- `s3-storage` — S3-compatible storage backend
- `profiling` — backend tracing-chrome + frontend IPC instrumentation

## Git Workflow
- Work on `staging`, merge to `main` for releases
- Version in `src-tauri/Cargo.toml` + `src-tauri/tauri.conf.json` (keep in sync)
- Push to `main` triggers auto-tag → release workflow
- Release needs manual trigger: `gh workflow run release.yml --ref v{version}`

## Debugging Quick Ref
```bash
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log          # Live logs
grep "\[Frontend\]" ~/.local/share/app.hirsel/logs/Hirsel.log | tail -20  # Frontend errors
RUST_LOG=debug ./dev.sh                                     # Verbose backend
pkill -f "hirsel __daemon"                                  # Kill stuck daemon
```
