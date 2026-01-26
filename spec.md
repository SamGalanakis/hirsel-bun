# Hirsel Rewrite Specification

> **HISTORICAL:** This specification was for the original Rust/Tauri rewrite.
> The frontend has since migrated from Alpine.js to SolidJS.
> See `docs/architecture.md` for current architecture.

## Overview

Full rewrite of hirsel in Rust using [Tauri](https://tauri.app/) for the desktop app. The project has two deliverables:

1. **CLI Tool** - Full feature parity with current hirsel CLI (Rust binary)
2. **Native Desktop App** - Modern, playful UI replacing the current TUI live view

## IMPORTANT: Reference Implementation

**You MUST study the reference implementation at `references/hirsel/` thoroughly.**

This is the existing Python implementation that you are rewriting in Rust. Key files to study:

```
references/hirsel/
├── src/hirsel/
│   ├── cli.py           # CLI commands - port all of these
│   ├── live.py          # TUI dashboard - replace with Tauri GUI
│   ├── state.py         # SQLite state management - port this pattern
│   ├── config.py        # Configuration system
│   ├── git.py           # Git operations (worktrees, branches)
│   ├── workers.py       # Worker naming and management
│   ├── worker_runner.py # Worker spawning and lifecycle
│   ├── acp.py           # Agent Control Protocol client
│   ├── chats.py         # Chat/messaging system
│   ├── files.py         # File system utilities
│   └── schema.sql       # Database schema
```

**Read these files. Understand the architecture. Match the behavior.**

## Technology Stack

- **Language**: Rust
- **Desktop Framework**: Tauri v2
- **Database**: SQLite (via rusqlite)
- **Frontend**: Vanilla TypeScript + Alpine.js + Tailwind CSS + [Basecoat UI](https://basecoatui.com/)
- **CLI**: clap for argument parsing

### Why Basecoat UI?

Basecoat provides shadcn-quality components without React dependency. It's framework-agnostic, built on Tailwind, and includes 40+ accessible components (cards, buttons, modals, tabs, etc.).

---

## Part 1: CLI Tool (Rust)

### Commands (Full Parity)

#### Run Management
| Command | Description | Options |
|---------|-------------|---------|
| `hirsel` | Launch native desktop app (default) | - |
| `hirsel go <run> <spec>` | Start a new run | `--workers N`, `--time-limit 30m` |
| `hirsel view <run>` | View run status | - |
| `hirsel log <run>` | View activity log | `-f` (follow) |
| `hirsel attach <run>` | Watch worker live | Worker selection menu |
| `hirsel msg <run> <msg>` | Send message to run | - |
| `hirsel diff <run>` | Show code changes | - |
| `hirsel deliver <run>` | Create branch in target repo | - |
| `hirsel pause <run>` | Pause workers | - |
| `hirsel resume <run>` | Resume paused/timed-out run | `--time-limit` |
| `hirsel delete <run>` | Remove a run | - |
| `hirsel prune` | Remove all delivered runs | - |
| `hirsel runs` | List all runs | `--json` |
| `hirsel summary <run>` | Generate run summary | - |

#### Task Management
| Command | Description |
|---------|-------------|
| `hirsel task-add <run> <id> <desc>` | Add task to run |
| `hirsel task-delete <run> <id>` | Delete task |
| `hirsel task-done <run> <id>` | Mark task done |
| `hirsel task-reopen <run> <id>` | Reopen completed task |
| `hirsel task-unclaim <run> <id>` | Unclaim task |
| `hirsel tasks <run>` | List tasks |

#### Configuration
| Command | Description |
|---------|-------------|
| `hirsel config` | Interactive agent selection |
| `hirsel config <agent>` | Set agent (claude, gemini, opencode, codex, goose) |
| `hirsel templates` | Manage spec templates |
| `hirsel completions` | Shell completion scripts |
| `hirsel spec` | Spec management |
| `hirsel improve` | Update project memory from learnings |
| `hirsel man` | Show manual |

#### Worker Commands (for AI agents via MCP)
| Command | Description |
|---------|-------------|
| `hirsel-worker task-claim <id>` | Claim a task |
| `hirsel-worker task-done <id>` | Complete a task |
| `hirsel-worker task-add <id> <desc>` | Add follow-up task |
| `hirsel-worker task-await` | Wait for available tasks |
| `hirsel-worker msg <thread> <msg>` | Send message |

### Rust Project Structure

```
src-tauri/
├── src/
│   ├── main.rs              # Entry point (CLI or GUI mode)
│   ├── lib.rs               # Shared library for Tauri + CLI
│   ├── cli/
│   │   ├── mod.rs           # CLI command routing (clap)
│   │   ├── go.rs
│   │   ├── view.rs
│   │   ├── log.rs
│   │   ├── attach.rs
│   │   ├── msg.rs
│   │   ├── diff.rs
│   │   ├── deliver.rs
│   │   ├── pause.rs
│   │   ├── resume.rs
│   │   ├── delete.rs
│   │   ├── prune.rs
│   │   ├── runs.rs
│   │   ├── summary.rs
│   │   ├── config.rs
│   │   ├── tasks.rs
│   │   ├── improve.rs
│   │   └── man.rs
│   ├── core/
│   │   ├── mod.rs
│   │   ├── state.rs         # SQLite state management (rusqlite)
│   │   ├── config.rs        # Configuration system
│   │   ├── git.rs           # Git operations (git2 crate)
│   │   ├── workers.rs       # Worker spawning and management
│   │   ├── acp.rs           # Agent Control Protocol client
│   │   ├── files.rs         # File system utilities
│   │   └── chats.rs         # Chat/messaging system
│   ├── worker/              # Worker subprocess commands
│   │   ├── mod.rs
│   │   ├── task_claim.rs
│   │   ├── task_done.rs
│   │   └── msg.rs
│   └── gui/
│       ├── mod.rs
│       └── commands.rs      # Tauri IPC commands
├── Cargo.toml
└── tauri.conf.json
```

### State Schema (SQLite)

```sql
-- Run state
CREATE TABLE state (
  id INTEGER PRIMARY KEY,
  status TEXT NOT NULL,
  request TEXT,
  project_path TEXT,
  worker_scale TEXT,
  time_limit_minutes INTEGER,
  started_at TEXT,
  summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Tasks
CREATE TABLE tasks (
  id TEXT PRIMARY KEY,
  description TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'todo',
  claimed_by TEXT,
  claimed_at TEXT,
  parent_id TEXT,
  blocked_by TEXT,
  tokens_used INTEGER,
  created_at TEXT NOT NULL
);

-- Workers
CREATE TABLE workers (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  pid INTEGER,
  session_id TEXT,
  status TEXT NOT NULL DEFAULT 'idle',
  work_dir TEXT,
  waiting_thread TEXT,
  location TEXT DEFAULT 'local',
  last_heartbeat TEXT,
  created_at TEXT NOT NULL
);

-- Evals
CREATE TABLE evals (
  id INTEGER PRIMARY KEY,
  branch TEXT NOT NULL,
  eval_name TEXT,
  status TEXT NOT NULL DEFAULT 'running',
  feedback TEXT,
  log_file TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT
);

-- Messages
CREATE TABLE messages (
  id INTEGER PRIMARY KEY,
  thread TEXT NOT NULL,
  sender TEXT NOT NULL,
  content TEXT NOT NULL,
  waiting INTEGER DEFAULT 0,
  read_by TEXT,
  timestamp TEXT NOT NULL
);

-- History
CREATE TABLE history (
  id INTEGER PRIMARY KEY,
  timestamp TEXT NOT NULL,
  action TEXT NOT NULL,
  detail TEXT
);
```

### Key Rust Crates

```toml
[dependencies]
tauri = { version = "2", features = ["shell-open"] }
clap = { version = "4", features = ["derive"] }
rusqlite = { version = "0.31", features = ["bundled"] }
git2 = "0.18"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
chrono = "0.4"
dirs = "5"
agent-client-protocol = "latest"  # ACP - for agent communication
```

### Agent Client Protocol (ACP)

Hirsel uses [ACP](https://agentclientprotocol.com/) to communicate with AI coding agents (Claude Code, etc.). ACP is a standardized protocol (like LSP for language servers) that enables editors/tools to interact with AI agents.

- **Crate**: `agent-client-protocol` (cargo add agent-client-protocol)
- **Communication**: JSON-RPC over stdin/stdout for local agents
- **Reference**: See `references/hirsel/src/hirsel/acp.py` for Python implementation

### Worker System

Workers are separate processes that run AI agents. Follow the Python implementation pattern:

1. **Local workers**: Spawned as subprocesses in tmux sessions
2. **Remote workers**: Spawned via SSH on remote machines (see `references/hirsel/src/hirsel/remote.py`)
3. **Git isolation**: Each worker gets its own git worktree for parallel work
4. **Heartbeat**: Workers send periodic heartbeats to track liveness

Key files to study:
- `references/hirsel/src/hirsel/worker_runner.py` - Spawning and lifecycle
- `references/hirsel/src/hirsel/remote.py` - SSH-based remote spawning
- `references/hirsel/src/hirsel/git.py` - Git worktree management

### Eval System

The eval system runs automated checks on worker output. Feature parity with Python required.

- See `references/hirsel/src/hirsel/eval.py`
- Evals are tracked in the `evals` table
- Results feed back into the worker loop

---

## Part 2: Native Desktop App

### Overview

Replace the current Textual-based TUI (`live.py`) with a native Tauri application. The UI should be **modern and playful**, inspired by the "hirsel" (flock of sheep) theme — NOT a terminal interface.

### Frontend Stack

- **Vanilla TypeScript** - Logic, Tauri command calls
- **Alpine.js** - Declarative reactivity in HTML
- **Tailwind CSS** - Utility-first styling
- **Basecoat UI** - Pre-built accessible components (cards, buttons, modals, tabs, etc.)

Install with:
```bash
npm install basecoat-css tailwindcss alpinejs @tauri-apps/api
```

CSS setup:
```css
@import "tailwindcss";
@import "basecoat-css";
```

JS setup (for interactive components):
```typescript
import 'basecoat-css/all';  // or cherry-pick: 'basecoat-css/popover'
```

### Frontend Structure

```
src/
├── index.html              # Main HTML with Alpine components
├── main.ts                 # Entry point, Tauri invoke calls
├── styles/
│   └── main.css            # Tailwind + custom styles
├── components/             # Alpine component functions
│   ├── run-list.ts
│   ├── run-detail.ts
│   ├── worker-panel.ts
│   ├── task-panel.ts
│   ├── activity-log.ts
│   ├── chat-panel.ts
│   └── status-bar.ts
└── lib/
    ├── api.ts              # Tauri invoke wrappers
    ├── types.ts            # TypeScript interfaces
    └── theme.ts            # Color constants
```

### UI Layout

```
┌─────────────────────────────────────────────────────────────────┐
│  Hirsel                                              [─][□][×]  │
├─────────────────────────────────────────────────────────────────┤
│ ┌─────────────┬───────────────────────────────────────────────┐ │
│ │  Runs       │  Run Details                                  │ │
│ │             │                                               │ │
│ │  ● myrun    │  ┌─────────────────┬─────────────────────────┐│ │
│ │  ○ oldrun   │  │ Workers         │ Tasks                   ││ │
│ │             │  │                 │                         ││ │
│ │             │  │ ├─● alpha       │ ✓ scope                 ││ │
│ │             │  │ └─◌ beta        │ ► implement_feature     ││ │
│ │             │  │                 │ ○ write_tests           ││ │
│ │             │  │                 │                         ││ │
│ │             │  ├─────────────────┴─────────────────────────┤│ │
│ │             │  │ Activity                                  ││ │
│ │             │  │                                           ││ │
│ │             │  │ 14:32 alpha ✓ scope                       ││ │
│ │             │  │ 14:31 alpha ► implement_feature           ││ │
│ │             │  │ 14:30 → run working                       ││ │
│ │             │  └───────────────────────────────────────────┘│ │
│ └─────────────┴───────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│ Status: Working │ 2 workers │ 1/3 tasks │ 15m elapsed          │
└─────────────────────────────────────────────────────────────────┘
```

### Features

#### Run List Panel
- List all runs with status indicators
- Visual status: working (animated), waiting, done, idle
- Progress indicator (tasks done/total)
- Elapsed time
- Click to select
- Context menu: pause, resume, delete, deliver

#### Run Detail View
- **Header**: Run name, status badge, elapsed time, branch name
- **Workers Section**:
  - Card-based or list view showing workers
  - Status indicator per worker (working, waiting, done)
  - Leader badge (★)
  - Context utilization percentage
  - Click to attach → opens **embedded terminal view** (small, clean, scrollable)
- **Tasks Section**:
  - Clean task list with visual hierarchy
  - Status icons: done, doing, todo
  - Claimed by indicator
  - Right-click to manage tasks
- **Activity Log**:
  - Scrolling log of recent activity
  - Task claims, completions, status changes
  - Timestamps

**Note**: Diff view is out of scope for initial version. Use `hirsel diff <run>` CLI for now.

#### Chat Panel (slide-out)
- Thread list (user, group, workers)
- Message history
- Unread indicators
- Send message input

#### Status Bar
- Current run status
- Worker count (active/total)
- Task progress
- Time remaining (if time limit set)

### Alpine.js Pattern

```html
<!-- Run list with Alpine reactivity -->
<div x-data="runList()" x-init="init()">
  <template x-for="run in runs" :key="run.name">
    <div
      @click="selectRun(run.name)"
      :class="{ 'selected': selectedRun === run.name }"
      class="run-item"
    >
      <span class="status-dot" :class="run.status"></span>
      <span x-text="run.name"></span>
      <span class="progress" x-text="run.tasksDone + '/' + run.tasksTotal"></span>
    </div>
  </template>
</div>
```

```typescript
// components/run-list.ts
import { invoke } from '@tauri-apps/api/core';
import type { RunSummary } from '../lib/types';

export function runList() {
  return {
    runs: [] as RunSummary[],
    selectedRun: null as string | null,

    async init() {
      await this.refresh();
      setInterval(() => this.refresh(), 2000);
    },

    async refresh() {
      this.runs = await invoke('get_runs');
    },

    async selectRun(name: string) {
      this.selectedRun = name;
      window.dispatchEvent(new CustomEvent('run-selected', { detail: name }));
    }
  };
}
```

### Tauri Commands (Rust → Frontend)

```rust
// src-tauri/src/gui/commands.rs

#[tauri::command]
async fn get_runs() -> Result<Vec<RunSummary>, String> {
    // Read from ~/.hirsel/ databases
}

#[tauri::command]
async fn get_run_detail(name: String) -> Result<RunDetail, String> {
    // Get full run details
}

#[tauri::command]
async fn pause_run(name: String) -> Result<(), String> {
    // Pause workers
}

#[tauri::command]
async fn resume_run(name: String, time_limit: Option<String>) -> Result<(), String> {
    // Resume run
}

#[tauri::command]
async fn send_message(run: String, thread: String, message: String) -> Result<(), String> {
    // Send message to run
}

#[tauri::command]
async fn attach_worker(run: String, worker: String) -> Result<(), String> {
    // Open terminal with tmux attach
}
```

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j/k` | Navigate run list |
| `Enter` | Select run / expand detail |
| `a` | Attach to selected worker |
| `p` | Pause selected run |
| `r` | Resume selected run |
| `c` | Open chat panel |
| `m` | Send message |
| `n` | New run (opens dialog) |
| `?` | Show help |
| `Esc` | Close panel / deselect |

### Real-time Updates

- Poll state every 2 seconds via Tauri commands
- Debounce UI updates with Alpine reactivity
- Git info cached with 3s TTL

---

## Visual Design & Theme

### Concept: Herding Agents

The name "hirsel" is a Scottish term for a flock of sheep. The app should evoke the feeling of a shepherd overseeing their flock — **modern, warm, and playful** — NOT a terminal interface.

### Design Principles

1. **Modern & Clean** - Rounded corners, subtle shadows, smooth animations
2. **Warm & Organic** - Not cold/clinical tech aesthetic. Think wool, meadows, dusk.
3. **Playful Confidence** - Watching agents work should feel satisfying, not stressful
4. **Clear Hierarchy** - Easy to see which "sheep" need attention at a glance
5. **Subtle Animation** - Gentle movement suggests life without distraction

### Color Palette (Tailwind Custom)

```javascript
// tailwind.config.js
colors: {
  // Backgrounds
  pasture: {
    900: '#1a1a1a',  // darkest - main bg
    800: '#242424',  // panels
    700: '#2d2d2d',  // hover states
    600: '#333333',  // borders
  },

  // Text (warm off-white, like wool)
  wool: {
    100: '#e8e4df',  // primary text
    300: '#b5b0a8',  // secondary
    500: '#8a8580',  // muted
    700: '#5a5550',  // dim
  },

  // Accent (shepherd's lantern)
  amber: {
    400: '#e8c19a',  // bright
    500: '#d4a574',  // primary accent
    600: '#b8895c',  // hover
  },

  // Status colors
  sage: '#7d9970',     // success/done (healthy pasture)
  golden: '#c9a227',   // warning/waiting (attention needed)
  terra: '#c45c4a',    // error (problem)
}
```

### Typography

- **UI text**: System font stack (native feel)
- **Monospace** (logs, code): `"JetBrains Mono", "Fira Code", monospace`

### Visual Elements

- **Cards**: Rounded corners (8px), subtle shadow, hover lift effect
- **Status indicators**: Soft glowing dots, not harsh circles
- **Progress bars**: Rounded, gradient fills
- **Transitions**: 150-200ms ease-out for all interactions
- **Empty states**: Friendly sheep illustration, warm messaging

### The Sheep Theme

- Subtle sheep silhouette or wool texture in empty states
- Loading state: gentle animation (NOT spinning)
- Playful but professional — hint at the theme, don't make it cartoonish
- Consider a small sheep icon in the app header

### Theme System

- **Dark mode** (default): The warm "pasture at dusk" palette above
- **Light mode**: Inverted, with cream/white backgrounds and the same accent colors
- **Theme toggle**: User can switch between dark/light
- **Basecoat themes**: Leverage Basecoat's built-in theme system for consistency

### Window Chrome

- **Custom title bar** - Fits the hirsel aesthetic, not default OS chrome
- **Frameless window** with custom close/minimize/maximize buttons
- **Consistent across platforms** - Same look on macOS, Linux, Windows

### Empty States

- **Sheep illustrations** - Playful but professional SVG sheep graphics
- Not cartoonish - subtle, warm, inviting
- Examples:
  - "No runs yet" → sheep grazing peacefully
  - "Loading" → gentle bobbing animation (grazing)
  - "Error" → concerned-looking sheep

### Notifications

- Use system notifications for important events (run complete, error, etc.)
- In-app toast notifications for transient feedback
- Don't spam - only notify on state changes that matter

### UI Polish Requirements

1. **Smooth transitions** - All state changes animated
2. **Hover states** - Subtle highlight, slight lift on cards
3. **Focus rings** - Amber glow for keyboard focus
4. **Loading states** - Skeleton screens or gentle fade
5. **Empty states** - Sheep illustrations with warm messaging
6. **Responsive layout** - Panels resize gracefully
7. **Modern friendly** - Primarily mouse-driven, but keyboard shortcuts work well

---

## Implementation Phases

### Phase 1: Project Setup
1. Initialize Tauri v2 project
2. Set up Rust workspace (CLI + GUI shared lib)
3. Configure Tailwind CSS build
4. Add Alpine.js

### Phase 2: Core Rust Library
1. Port state management (rusqlite)
2. Port configuration system
3. Port git utilities (git2)
4. Port ACP client

### Phase 3: CLI Commands
1. Implement clap command routing
2. Port all run management commands
3. Port task management commands
4. Port worker subprocess commands
5. Implement shell completions

### Phase 4: Worker System
1. Port worker spawning and lifecycle
2. Port eval system
3. Port remote worker support

### Phase 5: Desktop App - Foundation
1. Build basic HTML structure with Alpine
2. Implement Tauri commands
3. Style with Tailwind
4. Run list rendering

### Phase 6: Desktop App - Features
1. Run detail view with all panels
2. Worker and task management UI
3. Activity log with real-time updates
4. Chat panel implementation
5. Keyboard shortcuts

### Phase 7: Polish
1. Animations and transitions
2. Empty states with sheep illustrations
3. Error handling and edge cases
4. Performance optimization

---

## Quality Requirements

- **Type Safety**: Rust's type system, TypeScript strict mode
- **Error Handling**: Rust Result types, never panic in library code
- **No Unwrap**: Use `?` operator or proper error handling
- **Testing**: Unit tests for core logic

## File Naming Conventions

- Rust: snake_case for files and functions
- TypeScript: kebab-case for files, camelCase for functions
- Components: kebab-case files, function names match

## Build Output

- `hirsel` - Single CLI binary
- `Hirsel.app` / `hirsel.AppImage` / `Hirsel.exe` - Desktop app with bundled webview
