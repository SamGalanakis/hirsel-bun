# Fly.io Deployment Guide

Deploy Hirsel as a remote coordinator on Fly.io with scalable workers.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                            Fly.io                                     │
│  ┌─────────────────────┐    ┌─────────────────────────────┐          │
│  │  Coordinator App    │    │  Worker Machines (ephemeral) │          │
│  │  hirsel-coordinator │    │  hirsel-workers app          │          │
│  │  - HTTP API         │◄───│  - Spawned on demand         │          │
│  │  - SQLite (LiteFS)  │    │  - Auto-destroyed on done    │          │
│  │  - Run management   │    │  - User-specified images     │          │
│  └─────────┬───────────┘    └──────────────┬──────────────┘          │
│            │                               │                          │
│            │    ┌──────────────────────┐   │                          │
│            └───►│  Tigris Storage      │◄──┘                          │
│                 │  (S3-compatible)     │                              │
│                 │  - Workspace archives│                              │
│                 │  - Session snapshots │                              │
│                 └──────────────────────┘                              │
└──────────────────────────────────────────────────────────────────────┘
         ▲
         │ HTTPS
         │
    ┌────┴────┐
    │  CLI    │  hirsel go my-run spec.md --profile fly
    └─────────┘
```

## Prerequisites

- [Fly CLI](https://fly.io/docs/hands-on/install-flyctl/) installed and authenticated
- Hirsel source code checked out

## Coordinator Setup

### 1. Create the App

```bash
fly apps create hirsel-coordinator
```

### 2. Attach Consul (for LiteFS)

LiteFS provides distributed SQLite, enabling coordinator scaling.

```bash
fly consul attach -a hirsel-coordinator
```

### 3. Create Tigris Storage

Tigris provides S3-compatible storage for snapshots and file sync (required for pause/resume with ephemeral workers).

```bash
fly storage create -a hirsel-coordinator --name hirsel-storage
```

This automatically sets `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT_URL_S3`, `AWS_REGION`, and `BUCKET_NAME` as secrets.

### 4. Set Secrets

```bash
# Generate an API key for authenticating requests
fly secrets set HIRSEL_API_KEY=$(openssl rand -hex 32) -a hirsel-coordinator
```

### 5. Build and Deploy

```bash
# Build the CLI binary with S3 storage support
cd src-tauri
cargo build --release --no-default-features --features cli,s3-storage

# Deploy
cd ..
fly deploy
```

The coordinator will be available at `https://hirsel-coordinator.fly.dev`

## Workers App Setup

Workers are ephemeral Fly Machines spawned on demand.

### 1. Create Workers App

```bash
fly apps create hirsel-workers
```

No further setup needed - machines are created dynamically by the coordinator.

## Local Configuration

Add a remote profile to your `~/.hirsel/config.toml`:

```toml
# Fly runner for ephemeral workers
[runners.fly]
[runners.fly.host]
type = "fly"
app = "hirsel-workers"
region = "ams"  # Optional: specific region
cpus = 2
memory_mb = 2048
[runners.fly.container]
image = "debian:bookworm-slim"  # Or your custom image

# Remote profile pointing to Fly coordinator
[profiles.fly]
mode = "remote"
url = "https://hirsel-coordinator.fly.dev"
api_key = "your-api-key-from-secrets"  # Same as HIRSEL_API_KEY
default_runner = "fly"
```

## Usage

```bash
# Start a run on Fly
hirsel go my-feature spec.md --profile fly

# Check status
hirsel view my-feature --profile fly

# Attach to see progress
hirsel attach my-feature --profile fly
```

## Custom Worker Images

You can use any Docker image for workers. The init script will:
1. Download hirsel worker binary from GitHub releases (version-matched)
2. Install Node.js if needed
3. Fetch project files from coordinator
4. Start the worker

Example with a pre-configured image:

```toml
[runners.fly.container]
image = "ghcr.io/myorg/dev-environment:latest"
```

## Scaling

### Coordinator Scaling

The coordinator uses LiteFS for distributed SQLite. To add replicas:

```bash
fly scale count 2 -a hirsel-coordinator
```

LiteFS handles replication automatically with Consul-based leader election.

### Worker Scaling

Workers scale automatically based on run configuration:

```bash
# Start with multiple workers
hirsel go my-run spec.md --profile fly --workers 4
```

## Monitoring

```bash
# View coordinator logs
fly logs -a hirsel-coordinator

# View worker machines
fly machines list -a hirsel-workers

# Check coordinator status
curl https://hirsel-coordinator.fly.dev/health
```

## Costs

- **Coordinator**: ~$2-5/month for a shared-cpu-1x machine
- **Workers**: Pay per second while running (auto-destroyed when done)

## Troubleshooting

### Workers not spawning

Check that the workers app exists and you have FLY_API_TOKEN set:

```bash
fly apps list  # Should show hirsel-workers
fly auth token  # Verify you're authenticated
```

### Connection refused

Verify the coordinator is running:

```bash
fly status -a hirsel-coordinator
curl https://hirsel-coordinator.fly.dev/health
```

### LiteFS errors

Ensure Consul is attached:

```bash
fly consul attach -a hirsel-coordinator
```
