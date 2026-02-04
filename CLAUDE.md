# Hirsel Development Guidelines

## Code Quality Rules
- **NEVER** leave legacy code paths - consolidate everything
- **NEVER** add "TODO" or "FIXME" comments for future work
- **NEVER** leave deprecated or backward-compatibility code
- **NEVER** duplicate logic across CLI/GUI - use shared abstractions
- If you're refactoring, FINISH the refactor completely - no half measures

## Documentation

**Before starting work:** Run `ls docs/` and read relevant sections based on your task:

| Doc | When to Read |
|-----|--------------|
| `architecture.md` | **Always** - module map, traits, data flow, where to add code |
| `debugging.md` | Troubleshooting issues, understanding logs |
| `fly-deployment.md` | Remote/Fly.io runner work |
| `basecoatui.md` | Building UI components |
| `design-language.md` | Visual styling, colors, typography |
| `future.md` | Planned improvements (font, git2→gix migration) |

**CRITICAL:**
1. **READ** `docs/architecture.md` before implementing - use Quick Reference to find files
2. **UPDATE** `docs/architecture.md` when you add/modify modules, traits, or APIs

## No Backwards Compatibility
Development project - no database migrations needed. Modify schema directly in `src-tauri/src/core/state/mod.rs`.

**Data locations:**
```
~/.hirsel/              # Config, global DB, runs
~/.local/share/app.hirsel/  # Tauri logs
```

**Reset:** `rm -rf ~/.hirsel/runs` (runs only) or `rm -rf ~/.hirsel ~/.local/share/app.hirsel` (full)

## UI Stack

- **SolidJS** - Reactive UI framework (`src/components/`, `src/stores/`, `src/hooks/`)
- **Basecoat UI** - shadcn/ui patterns for vanilla HTML (https://basecoatui.com)
- **Lucide Icons** - https://lucide.dev/icons/
- **Tailwind CSS v4** with custom theme

```tsx
// Components: src/components/**/*.tsx
// Stores: src/stores/ (AppProvider, ProjectProvider, RunsProvider, SelectionProvider, DeltaProvider, RouteProvider)
// Hooks: src/hooks/ (usePolling, useDebounce, useTauriEvent)
```

**Icons:** Use the `<Icon name="icon-name" />` component from `src/components/shared`. Icons render as inline SVGs - no DOM mutation needed.

```tsx
import { Icon } from '../shared';
<Icon name="check" class="w-4 h-4 text-sage" />
```

**Toast:** `window.toast.success('msg')` / `window.toast.error('msg')`

## Development

```bash
./dev.sh                # Run with debug environment
cargo build             # Build to src-tauri/target/debug/hirsel
```

**CLI commands:** `runs`, `view <run>`, `attach <run>`, `go <run> <spec>`, `test <scenario> --yolo`, `tasks <project>`

## Debugging

See [`docs/debugging.md`](docs/debugging.md) for comprehensive debugging guide.

**Quick reference:**
```bash
# View logs in real-time
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log

# Frontend errors (in dev mode, console.error goes to this log)
grep "\[Frontend\]" ~/.local/share/app.hirsel/logs/Hirsel.log | tail -20

# Reset everything
rm -rf ~/.hirsel ~/.local/share/app.hirsel
```

## Dev Tools

Install dev tools:
```bash
cargo install cargo-deny cargo-machete cargo-nextest typos-cli tokei prek
npm install
```

Run checks:
```bash
npm run lint              # Biome lint
npm run lint:fix          # Biome lint + fix
npm run test              # cargo nextest
cargo deny check          # Dependency audit
cargo machete             # Find unused deps
typos                     # Spell check
tokei                     # LOC stats
```

**Pre-commit with [prek](https://github.com/j178/prek):**
```bash
prek install              # Install hooks
prek                      # Run on staged files
prek run --all-files      # Run on all files
```

Hooks: typos, cargo-fmt, cargo-check, cargo-clippy, cargo-deny, tsc, biome.

## Daemon

The daemon (`hirsel __daemon`) runs as a background process managing run lifecycle:
- Polls active runs every 5 seconds
- Triggers eval when all workers become inactive
- Enforces time limits
- Processes scribe batches

**Singleton pattern:** One daemon per user. PID file at `~/.hirsel/hirsel.pid` stores PID and binary path.

**Binary mismatch detection:** If daemon was started from a different binary (e.g., debug vs release), it auto-restarts with the current binary when `connect_or_start()` is called.

**Port:** Default 19700, configurable via `HIRSEL_DAEMON_PORT` env var (escape hatch if port conflicts).

## Process Management

Use `AcpChild` for spawning ACP processes - handles process groups and cleanup:
```rust
let mut acp_child = AcpChild::spawn(AcpSpawnConfig::new(cmd, dir, "context"))?;
// Cleanup automatic on drop (SIGTERM → wait → SIGKILL)
```

## Cargo Features

| Feature | Use |
|---------|-----|
| `gui` | Tauri desktop (default, includes `cli`) |
| `cli` | Full CLI (includes server + TUI attach) |
| `server` | HTTP server, daemon |
| `worker` | Minimal remote worker |
| `s3-storage` | S3-compatible storage (MinIO, Tigris, AWS S3) |

```bash
cargo build                                    # Full GUI
cargo build --no-default-features -F cli       # CLI only
cargo build --no-default-features -F worker    # Remote worker
cargo build --features s3-storage              # With S3 storage support
```

## Fly.io

See [`docs/fly-deployment.md`](docs/fly-deployment.md) for full Fly.io deployment guide.

**Quick start:**
```bash
# Coordinator
fly apps create hirsel-coordinator
fly secrets set HIRSEL_API_KEY=<secret> ANTHROPIC_API_KEY=<key>
fly deploy

# Workers app (machines created on-demand)
fly apps create hirsel-workers

# Usage
hirsel go my-feature spec.md --profile fly
```

## Git Workflow

**Always work on `staging`, then merge to `main` for releases.**

```bash
git checkout staging           # All work happens here
# ... make changes, commit ...
git push origin staging        # Push to staging

# When ready to release:
git checkout main
git merge staging -m "Merge staging for v0.X.X release"
git push origin main           # Triggers auto-tag → release workflow
git checkout staging           # Return to staging
```

## Releases

Version is defined in `src-tauri/Cargo.toml`. Keep `src-tauri/tauri.conf.json` version in sync.

**Release flow:**
1. Update version in `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
2. Commit to `staging`, then merge `staging` → `main`
3. Auto-tag workflow (`.github/workflows/auto-tag.yml`) creates `v{version}` tag
4. Release workflow (`.github/workflows/release.yml`) builds and publishes
5. Manual trigger needed: `gh workflow run release.yml --ref v{version}` (GITHUB_TOKEN can't trigger workflows)

**What gets built:**
- `hirsel-cli-{version}-linux-amd64` - CLI + server (no GUI)
- `hirsel-worker-{version}-linux-amd64` - Minimal worker binary
- Tauri desktop apps: macOS (aarch64, x86_64), Linux (deb, AppImage, rpm)

**Pre-release tags:** Use `v{version}-rc.1` or `v{version}-staging.1` for non-main branches.

**Manual release:** Push tag directly: `git tag v0.4.0 && git push origin v0.4.0`

## Storage

File storage abstraction for local/cloud deployments. Default is local filesystem.

Config (`~/.hirsel/config.toml`):
```toml
# S3-compatible storage (MinIO, Tigris, AWS S3)
[storage]
files = "s3"

[storage.s3]
endpoint = "http://localhost:9000"  # MinIO URL
bucket = "hirsel"
region = "us-east-1"
access_key_id = "minioadmin"
secret_access_key = "minioadmin"
```

Test with MinIO:
```bash
docker run -p 9000:9000 -p 9001:9001 minio/minio server /data --console-address ":9001"
```
