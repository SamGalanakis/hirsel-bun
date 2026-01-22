# Hirsel Development Guidelines

## Code Quality Rules
- **NEVER** leave legacy code paths - consolidate everything
- **NEVER** add "TODO" or "FIXME" comments for future work
- **NEVER** leave deprecated or backward-compatibility code
- **NEVER** duplicate logic across CLI/GUI - use shared abstractions
- If you're refactoring, FINISH the refactor completely - no half measures

## Architecture
See [`docs/architecture.md`](docs/architecture.md) for system architecture. **Keep it updated.**

## No Backwards Compatibility
Development project - no database migrations needed. Modify schema directly in `src-tauri/src/core/state/mod.rs`.

**Data locations:**
```
~/.hirsel/              # Config, global DB, runs
~/.local/share/app.hirsel/  # Tauri logs
```

**Reset:** `rm -rf ~/.hirsel/runs` (runs only) or `rm -rf ~/.hirsel ~/.local/share/app.hirsel` (full)

## UI Stack

See [`docs/basecoatui.md`](docs/basecoatui.md) for full component reference.

- **Basecoat UI** - https://basecoatui.com/components/
- **Lucide Icons** - https://lucide.dev/icons/
- **Alpine.js** for reactivity
- **Tailwind CSS v4** with custom theme

Key patterns:
```html
<button class="btn">Primary</button>
<button class="btn-outline">Outline</button>
<div class="grid gap-2"><label>X</label><input /><p class="text-muted-foreground text-sm">Help</p></div>
<i data-lucide="icon-name" class="w-4 h-4"></i>
```

**Lucide caveat:** Wrap in `<span>` for Alpine directives; call `lucide.createIcons({ inTemplates: true })` after dynamic updates.

**Toast:** `window.toast.success('msg')` / `window.toast.error('msg')`

## Development

```bash
./dev.sh                # Run with debug environment
cargo build             # Build to src-tauri/target/debug/hirsel
```

**Logs:** `tail -f ~/.local/share/app.hirsel/logs/Hirsel.log`

**CLI commands:** `runs`, `view <run>`, `attach <run>`, `go <run> <spec>`, `test <scenario> --yolo`

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

## Process Management

Use `AcpChild` for spawning ACP processes - handles process groups and cleanup:
```rust
let mut acp_child = AcpChild::spawn(AcpSpawnConfig::new(cmd, dir, "context"))?;
// Cleanup automatic on drop (SIGTERM → wait → SIGKILL)
```

## Cargo Features

| Feature | Use |
|---------|-----|
| `gui` | Tauri desktop (default) |
| `full-cli` | All CLI commands |
| `server` | HTTP server, daemon |
| `tui` | Terminal UI (attach) |
| `worker` | Minimal remote worker |
| `s3-storage` | S3-compatible storage (MinIO, Tigris, AWS S3) |

```bash
cargo build                                    # Full GUI
cargo build --no-default-features -F full-cli  # CLI only
cargo build --no-default-features -F worker    # Remote worker
cargo build --features s3-storage              # With S3 storage support
```

## Sprites

Cloud VMs via [sprites.dev](https://sprites.dev). Setup checkpoint:
```bash
cargo build --release --no-default-features --features worker
SPRITES_TOKEN=xxx ./scripts/setup-sprite-checkpoint.sh my-checkpoint
```

Config (`~/.hirsel/config.toml`):
```toml
[runners.cloud]
[runners.cloud.host]
type = "sprite"
api_token = "..."
checkpoint = "checkpoint-id"
auto_destroy = true
```

## Fly.io

Deploy coordinator and workers on [Fly.io](https://fly.io).

### Coordinator Deployment

```bash
# One-time setup
fly apps create hirsel-coordinator
fly volumes create hirsel_data --size 10 --region ams
fly secrets set HIRSEL_API_KEY=<secret> ANTHROPIC_API_KEY=<key>

# Build and deploy
cargo build --release --no-default-features --features full-cli
fly deploy
```

### Worker App

```bash
# Create workers app (machines created on-demand)
fly apps create hirsel-workers
```

### Config

```toml
[runners.fly]
[runners.fly.host]
type = "fly"
app = "hirsel-workers"
region = "ams"
cpus = 2
memory_mb = 2048
[runners.fly.container]
image = "debian:bookworm-slim"

[profiles.fly]
mode = "remote"
url = "https://hirsel-coordinator.fly.dev"
api_key = "your-secret-key"
default_runner = "fly"
```

### Usage

```bash
hirsel go my-feature spec.md --profile fly
```

See `docs/architecture.md` for full deployment guide.

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
