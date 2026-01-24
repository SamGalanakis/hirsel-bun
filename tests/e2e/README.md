# E2E Tests for Remote Orchestration

This directory contains end-to-end tests for hirsel's remote execution capabilities using pytest.

## Quick Start

```bash
cd tests/e2e

# Install dependencies
uv sync

# Run fast tests (default: local runner, noop scenario)
uv run pytest

# Run with Claude API (requires ANTHROPIC_API_KEY in .env)
uv run pytest --scenario=hello_world --scenario=calculator

# Run specific runner
uv run pytest --runner=fly --runner=ssh

# Run specific scenario
uv run pytest --scenario=calculator

# Full matrix
uv run pytest --runner=local --runner=docker --runner=fly \
              --scenario=hello_world --scenario=calculator

# Use a profile (from config.toml)
uv run pytest --profile=local           # Local orchestrator (default)
uv run pytest --profile=fly-remote      # Remote Fly.io coordinator
uv run pytest --profile=my-ssh-server   # Remote via SSH

# Control worker scale per run
uv run pytest --workers=3               # Use 3 hirsel workers per run

# Use isolated hirsel directory (default: creates temp dir)
uv run pytest --hirsel-dir=/tmp/test-hirsel  # Use specific directory

# Run only fast tests
uv run pytest -m "not slow"

# Verbose with timeout override
uv run pytest -v --timeout=900
```

## Architecture

```
┌─────────────────────┐
│  Test Runner        │  (local machine with Claude OAuth)
│  tests/e2e/         │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────────────────────────────────────┐
│  Orchestrator (profile-based)                       │
│  - local: hirsel binary on local machine            │
│  - remote: existing remote coordinator (Fly, SSH)   │
└─────────────────────────────────────────────────────┘
          │
    ┌─────┴─────┬─────────────┬──────────┐
    ▼           ▼             ▼          ▼
┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐
│ Local  │  │ Docker │  │  SSH   │  │  Fly   │
│ Runner │  │ Runner │  │ Runner │  │ Runner │
└────────┘  └────────┘  └────────┘  └────────┘
```

## Profiles

Profiles determine where the orchestrator runs. Configure in `~/.hirsel/config.toml`:

```toml
[profiles.local]
mode = "local"

[profiles.fly-remote]
mode = "remote"
url = "https://hirsel-coordinator.fly.dev"
api_key = "secret"

[profiles.my-ssh-server]
mode = "remote"
url = "http://my-server:8080"
api_key = "secret"
```

Use with `--profile=<name>`. Default is `local`.

## Runners

| Runner | Description | Requirements |
|--------|-------------|--------------|
| `local` | Runs directly on orchestrator | None |
| `docker` | Runs in Docker container | Docker daemon |
| `ssh` | Runs via SSH | SSH access |
| `fly` | Runs on Fly.io machine | `FLY_API_TOKEN` |

## Prerequisites

**Environment variables** (loaded from `.env` in `tests/e2e/` or project root):

```bash
# tests/e2e/.env
ANTHROPIC_API_KEY=sk-ant-...      # For Claude API
FLY_API_TOKEN=...                  # For fly runner
FLY_WORKERS_APP=hirsel-workers     # Fly.io workers app name
```

**Binary (one of the following):**
- Set `HIRSEL_BINARY` to path of hirsel binary
- Build locally: `cd src-tauri && cargo build --release --no-default-features`

**Credentials:**
- Local Claude OAuth (from `~/.claude/.credentials.json`) - forwarded automatically
- Or `ANTHROPIC_API_KEY` from `.env`

## Test Structure

```
tests/e2e/
├── pyproject.toml          # uv project config
├── conftest.py             # pytest fixtures
├── test_runs.py            # Main parametrized tests
├── orchestrator.py         # Orchestrator setup
├── runners/                # Runner implementations
│   ├── base.py
│   ├── local.py
│   ├── docker.py
│   ├── ssh.py
│   └── fly.py
└── README.md
```

## Adding a New Runner

1. Create `runners/myrunner.py`:

```python
from .base import BaseRunner, RunnerConfig

class MyRunner(BaseRunner):
    config = RunnerConfig(
        name="myrunner",
        timeout=600,
        markers=["slow"],
    )

    def skip_if_unavailable(self) -> None:
        # Check prerequisites, call pytest.skip() if not met
        pass

    def configure(self, orchestrator) -> None:
        # Write config to orchestrator.write_config()
        pass

    def verify_output(self, orchestrator, run_name, scenario) -> None:
        # Verify scenario output
        pass
```

2. Register in `runners/__init__.py`
3. Add to `RUNNERS` dict in `conftest.py`

## Markers

- `slow` - Tests that take significant time (fly runners)
- `docker` - Tests requiring Docker
- `fly` - Tests requiring fly CLI

## CI Integration

```yaml
# .github/workflows/e2e.yml
name: E2E Tests
on:
  workflow_dispatch:  # Manual trigger (costs money)

jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build CLI
        run: cd src-tauri && cargo build --release --no-default-features
      - name: Install uv
        uses: astral-sh/setup-uv@v4
      - name: Run E2E
        env:
          FLY_API_TOKEN: ${{ secrets.FLY_API_TOKEN }}
        run: |
          cd tests/e2e
          uv sync
          uv run pytest --runner=local --runner=docker -v
```
