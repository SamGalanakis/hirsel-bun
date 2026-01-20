# E2E Tests for Remote Orchestration

This directory contains end-to-end tests for hirsel's remote execution capabilities.

## Architecture

```
┌─────────────────────┐
│  Test Runner        │  (local machine with Claude OAuth)
│  tests/e2e/         │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────────────────────────────────────┐
│  Sprite #1: Orchestrator Server                     │
│  - hirsel serve --port 8080                         │
│  - HIRSEL_API_KEY=<generated>                       │
│  - Receives forwarded credentials from test runner  │
└─────────────────────────────────────────────────────┘
          │
    ┌─────┴─────┬─────────────┐
    ▼           ▼             ▼
┌────────┐  ┌────────┐  ┌──────────┐
│ Local  │  │Sprite 2│  │SSH back  │
│ Runner │  │ Runner │  │to Sprite1│
└────────┘  └────────┘  └──────────┘
```

## Prerequisites

**Required credentials:**
- `SPRITES_TOKEN` - sprites.dev API access
- Local Claude OAuth (from `~/.claude/.credentials.json`) - forwarded automatically

**Binary (one of the following):**
- Set `HIRSEL_RELEASE_URL` to a public download URL for the binary
- Configure SSH access to sprites.dev (for SCP upload)
- Build locally: `cd src-tauri && cargo build --release --no-default-features`

## Test Scenarios

### test_server_deploy.sh
Deploys hirsel server to a sprites.dev machine and verifies health check.

### test_local_runner.sh
Runs hello_world on the orchestrator server itself using local runner.

### test_sprite_runner.sh
Runs hello_world on a second sprites.dev machine.

### test_ssh_runner.sh
Runs hello_world via SSH connection back to the server sprite.

## Running Tests

```bash
# Set required environment variable
export SPRITES_TOKEN="your-token"

# Option A: Use a release URL (recommended)
export HIRSEL_RELEASE_URL="https://github.com/your-org/hirsel/releases/download/v1.0.0/hirsel-linux-x64"

# Option B: Use SCP (requires sprites.dev SSH access)
# No extra config needed if SSH is set up

# Run all tests
./tests/e2e/run_all.sh

# Run individual test
./tests/e2e/test_server_deploy.sh

# Run with custom scenario (default: hello_world)
TEST_SCENARIO=calculator ./tests/e2e/test_local_runner.sh

# Override binary location (for SCP method)
HIRSEL_BINARY=/path/to/hirsel ./tests/e2e/run_all.sh
```

## Credentials Flow

```
Local Machine                    Server Sprite                Worker
─────────────────────────────────────────────────────────────────────
~/.claude/.credentials.json
        │
        ▼
ForwardedCredentials {
  claude_access_token: "..."
}
        │
        │  (HTTP header or request body)
        ▼
Orchestrator receives creds
        │
        │  (env vars on spawn)
        ▼
CLAUDE_ACCESS_TOKEN=...
Worker process uses OAuth
```

## CI Integration

These tests are designed for manual or CI execution:

```yaml
# .github/workflows/e2e.yml
name: E2E Tests
on:
  workflow_dispatch:  # Manual trigger only (costs money)

jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build CLI
        run: cd src-tauri && cargo build --release --no-default-features
      - name: Run E2E
        env:
          SPRITES_TOKEN: ${{ secrets.SPRITES_TOKEN }}
        run: ./tests/e2e/run_all.sh
```

## Cleanup

Each test cleans up its sprites on exit (success or failure). If a test is interrupted, sprites may remain running. Check sprites.dev dashboard to manually clean up.
