# Hirsel

**Herd your AI coding agents**

Hirsel is a desktop application and CLI tool for orchestrating multiple AI coding agents working together on software projects. It provides a visual interface for managing runs, monitoring worker progress, and coordinating tasks across a team of AI agents.

## Features

- **Multi-agent orchestration** - Run multiple AI agents in parallel, each working on different tasks
- **Task management** - Hierarchical task breakdown with dependencies and blocking relationships
- **Real-time monitoring** - Watch agent progress, token usage, and context utilization
- **Team communication** - Built-in chat for human-in-the-loop coordination
- **Git integration** - Automatic branch management and diff tracking
- **Evaluation system** - Run automated evaluations against your specifications
- **Flexible runners** - Support for local execution and remote cloud runners

## Installation

### From Source

Requirements:
- Rust (latest stable)
- Bun or Node.js
- Tauri CLI

```bash
# Clone the repository
git clone https://github.com/yourusername/hirsel.git
cd hirsel

# Install dependencies
bun install

# Build for development
./dev.sh

# Build for production
bun run build
```

### Pre-built Packages

Download the latest release for your platform:
- `.deb` - Debian/Ubuntu
- `.rpm` - Fedora/RHEL
- `.AppImage` - Universal Linux

## Quick Start

### GUI

Launch the Hirsel application. The GUI provides:

- **Runs panel** - Create and manage agent runs
- **Overview tab** - Monitor workers and activity in real-time
- **Tasks tab** - View and manage the task breakdown
- **Specs tab** - Review specification and evaluation criteria
- **Evals tab** - View evaluation results
- **Chat tab** - Communicate with agents

### CLI

```bash
# Start a new run with a spec file
hirsel go my-feature spec.md

# List all runs
hirsel runs

# Watch live output
hirsel attach my-feature

# Send a message to agents
hirsel msg my-feature "Please prioritize the auth module"

# View changes
hirsel diff my-feature

# Deliver to a branch
hirsel deliver my-feature --branch feature/my-feature
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `go` | Start a new run |
| `runs` | List all runs |
| `view` | View run status |
| `log` | View activity log |
| `attach` | Watch worker live output (TUI) |
| `msg` | Send message to run |
| `diff` | Show code changes |
| `deliver` | Create branch in target repo |
| `pause` | Pause all workers |
| `resume` | Resume a paused run |
| `delete` | Remove a run |
| `tasks` | List tasks |
| `task-add` | Add a task |
| `task-done` | Mark task complete |
| `summary` | Generate run summary |
| `config` | Configure agent settings |

Run `hirsel --help` for full documentation.

## Configuration

Configure your AI agent in `~/.hirsel/config.toml`:

```toml
[agent]
command = ["claude", "code", "--acp"]  # or your preferred agent

[defaults]
workers = 3
mode = "yolo"  # or "hitl" for human-in-the-loop
```

Run `hirsel config` for interactive configuration.

## Development

```bash
# Run in development mode with hot reload
./dev.sh

# Run tests
cargo test

# Check formatting
cargo fmt --check

# Run linter
cargo clippy
```

### Project Structure

```
hirsel/
├── src/                 # Frontend (SolidJS + TypeScript)
│   ├── components/      # UI components (.tsx)
│   ├── stores/          # State management
│   ├── hooks/           # Custom hooks
│   └── styles/          # Tailwind CSS
├── src-tauri/           # Backend (Rust, Tauri)
│   └── src/
│       ├── cli/         # CLI commands
│       ├── core/        # Core logic
│       ├── gui/         # Tauri commands
│       └── worker/      # Agent worker management
└── docs/                # Architecture and debugging docs
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Author

**Samouil Galanakis**

---

*Hirsel: Because herding AI agents shouldn't feel like herding cats.*
