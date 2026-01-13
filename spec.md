# Hirsel Rewrite Specification

## Overview

Full rewrite of hirsel using [Electrobun](https://blackboard.sh/electrobun/docs/) as the desktop framework. The project has two deliverables:

1. **CLI Tool** - Full feature parity with current hirsel CLI
2. **Native Desktop App** - Polished version of the current TUI live view as a native application

## IMPORTANT: Reference Implementation

**You MUST study the reference implementation at `references/hirsel/` thoroughly.**

This is the existing Python implementation that you are rewriting in TypeScript/Bun. Key files to study:

```
references/hirsel/
├── src/hirsel/
│   ├── cli.py           # CLI commands - port all of these
│   ├── live.py          # TUI dashboard - port to native Electrobun app
│   ├── state.py         # SQLite state management - port this pattern
│   ├── config.py        # Configuration system
│   ├── git.py           # Git operations (worktrees, branches)
│   ├── workers.py       # Worker naming and management
│   ├── worker_runner.py # Worker spawning and lifecycle
│   ├── acp.py           # Agent Control Protocol client
│   ├── chats.py         # Chat/messaging system
│   ├── files.py         # File system utilities
│   ├── theme.py         # Colors, icons, styling (use NEW theme from spec)
│   ├── ui.py            # UI utilities
│   ├── eval.py          # Evaluation system
│   └── schema.sql       # Database schema
```

**Read these files. Understand the architecture. Match the behavior.**

## Technology Stack

- **Runtime**: Bun (TypeScript)
- **Desktop Framework**: Electrobun
- **Database**: SQLite (via bun:sqlite)
- **UI**: Native webview with custom components

---

## Part 1: CLI Tool

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

### CLI Architecture

```
src/
├── cli/
│   ├── index.ts          # Main CLI entry point (cyclopts equivalent)
│   ├── commands/
│   │   ├── go.ts
│   │   ├── view.ts
│   │   ├── log.ts
│   │   ├── attach.ts
│   │   ├── msg.ts
│   │   ├── diff.ts
│   │   ├── deliver.ts
│   │   ├── pause.ts
│   │   ├── resume.ts
│   │   ├── delete.ts
│   │   ├── prune.ts
│   │   ├── runs.ts
│   │   ├── summary.ts
│   │   ├── config.ts
│   │   ├── tasks.ts
│   │   ├── improve.ts
│   │   └── man.ts
│   └── worker/           # Worker subprocess commands
│       ├── task-claim.ts
│       ├── task-done.ts
│       └── msg.ts
├── core/
│   ├── state.ts          # SQLite state management
│   ├── config.ts         # Configuration (pydantic-settings equivalent)
│   ├── git.ts            # Git operations (worktrees, branches)
│   ├── workers.ts        # Worker spawning and management
│   ├── acp.ts            # Agent Control Protocol client
│   ├── files.ts          # File system utilities
│   └── chats.ts          # Chat/messaging system
└── shared/
    ├── types.ts          # Shared TypeScript types
    └── theme.ts          # Colors, icons, styling
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

---

## Part 2: Native Desktop App

### Overview

Replace the current Textual-based TUI (`live.py`) with a native Electrobun application providing a superior user experience.

### Window Structure

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

### App Architecture

```
src/
├── bun/                    # Main process (Bun)
│   ├── index.ts            # Entry point, window creation
│   ├── state-bridge.ts     # SQLite state to RPC bridge
│   ├── git-bridge.ts       # Git operations bridge
│   ├── worker-bridge.ts    # Worker management bridge
│   └── ipc.ts              # RPC handlers
├── views/
│   └── main/               # Main webview
│       ├── index.html
│       ├── index.ts        # View entry point
│       ├── components/
│       │   ├── RunList.ts
│       │   ├── RunDetail.ts
│       │   ├── WorkerList.ts
│       │   ├── TaskList.ts
│       │   ├── ActivityLog.ts
│       │   ├── ChatPanel.ts
│       │   └── StatusBar.ts
│       └── styles/
│           └── main.css
└── shared/
    ├── types.ts
    └── rpc-types.ts        # RPC message types
```

### Features

#### Run List Panel
- List all runs with status icons
- Status: ● working, ◌ waiting, ✓ done, ○ idle
- Progress indicator (tasks done/total)
- Elapsed time
- Click to select, double-click to attach
- Context menu: pause, resume, delete, deliver

#### Run Detail View
- **Header**: Run name, status badge, elapsed time, branch name
- **Workers Section**:
  - Tree view showing workers under run
  - Status indicator per worker (working, waiting, done)
  - Leader badge (★)
  - Context utilization percentage
  - Location for remote workers
  - Click to attach to worker
- **Tasks Section**:
  - Hierarchical task list
  - Status icons: ✓ done, ► doing, ○ todo
  - Claimed by indicator
  - Blocked by indicator
  - Right-click to manage tasks
- **Activity Log**:
  - Scrolling log of recent activity
  - Task claims, completions, status changes
  - Timestamps
- **Git Panel** (collapsible):
  - Branch graph visualization
  - Unmerged branches list

#### Chat Panel (slide-out)
- Thread list (user, group, workers)
- Message history
- Unread indicators
- Send message input
- Support for system messages

#### Status Bar
- Current run status
- Worker count (active/total)
- Task progress
- Token usage (input/output)
- Time remaining (if time limit set)

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j/k` | Navigate run list |
| `Enter` | Select run / expand detail |
| `a` | Attach to selected worker |
| `p` | Pause selected run |
| `r` | Resume selected run |
| `d` | Show diff |
| `c` | Open chat panel |
| `m` | Send message |
| `n` | New run (opens dialog) |
| `?` | Show help |
| `q` | Quit |

### RPC Interface

```typescript
// Main process -> Webview
interface StateUpdate {
  type: 'state_update';
  runs: Run[];
  selectedRun: RunDetail | null;
}

// Webview -> Main process
interface Commands {
  selectRun(name: string): void;
  pauseRun(name: string): void;
  resumeRun(name: string): void;
  deleteRun(name: string): void;
  attachWorker(run: string, worker: string): void;
  sendMessage(run: string, thread: string, msg: string): void;
  createRun(name: string, spec: string, options: RunOptions): void;
}
```

### Real-time Updates

- Poll state every 1 second
- Debounce UI updates
- Efficient diffing to minimize re-renders
- Git info cached with 3s TTL
- Metrics cached with 5s TTL

---

## Implementation Phases

### Phase 1: Core Infrastructure
1. Set up Electrobun project structure
2. Port state management (SQLite schema + operations)
3. Port configuration system
4. Port git utilities

### Phase 2: CLI Commands
1. Implement command routing with argument parsing
2. Port all run management commands
3. Port task management commands
4. Port worker subprocess commands
5. Implement shell completions

### Phase 3: Worker System
1. Port ACP client for agent communication
2. Port worker spawning and lifecycle management
3. Port eval system
4. Port remote worker support

### Phase 4: Desktop App - Foundation
1. Create BrowserWindow with proper frame settings
2. Implement state bridge (SQLite -> RPC)
3. Build basic component structure
4. Implement run list rendering

### Phase 5: Desktop App - Features
1. Run detail view with all panels
2. Worker and task management UI
3. Activity log with real-time updates
4. Chat panel implementation
5. Keyboard shortcuts

### Phase 6: Polish
1. Theming and styling refinement
2. Error handling and edge cases
3. Performance optimization
4. Native notifications
5. System tray support (optional)

---

## Visual Design & Theme

### Concept: Herding Agents

The name "hirsel" is a Scottish term for a flock of sheep. The app should evoke the feeling of a shepherd overseeing their flock - calm, organized, pastoral.

### Design Principles

1. **Warm & Organic** - Not cold/clinical tech aesthetic. Think wool, meadows, dusk.
2. **Calm Confidence** - Watching agents work should feel peaceful, not anxiety-inducing
3. **Clear Hierarchy** - Easy to see which sheep need attention at a glance
4. **Subtle Animation** - Gentle movement suggests life without distraction

### Color Palette

```
Background:     #1a1a1a (dark charcoal, like evening pasture)
Surface:        #242424 (slightly lighter panels)
Border:         #333333 (subtle separation)

Text Primary:   #e8e4df (warm off-white, like wool)
Text Secondary: #8a8580 (muted warmth)
Text Dim:       #5a5550

Accent:         #d4a574 (warm amber/gold - shepherd's lantern)
Accent Bright:  #e8c19a

Success:        #7d9970 (sage green - healthy pasture)
Warning:        #c9a227 (golden yellow - attention needed)
Error:          #c45c4a (terracotta red - problem)

Working:        #d4a574 (amber glow - active)
Waiting:        #c9a227 (needs attention)
Done:           #7d9970 (completed, at peace)
Idle:           #5a5550 (resting)
```

### Typography

- System native fonts for performance
- Monospace for code/logs: `"JetBrains Mono", "Fira Code", monospace`
- UI text: system default

### Iconography

```
Worker States:
  ● working (filled, glowing)
  ◐ waiting (half, attention)
  ○ idle (empty, resting)
  ✓ done (checkmark)

Task States:
  ► doing (in progress)
  ○ todo (pending)
  ✓ done (complete)

Run States:
  ● active run
  ◌ paused
  ✓ delivered
```

### The Sheep

- ASCII sheep in CLI banner (current logo is good)
- Desktop app: subtle sheep silhouette or wool texture in empty states
- Loading state: gentle bobbing animation (sheep grazing)
- Don't overdo it - hint at the theme, don't make it cartoonish

### UI Polish Requirements

1. **Smooth transitions** - Panel focus changes, list selection
2. **Hover states** - Subtle highlight on interactive elements
3. **Focus rings** - Clear keyboard focus indicators (amber)
4. **Loading states** - Skeleton screens or gentle spinners
5. **Empty states** - Friendly messaging ("No runs yet. Start herding with `hirsel go`")
6. **Responsive layout** - Panels resize gracefully
7. **Native feel** - Respect OS conventions (traffic lights on macOS, etc.)

---

## Quality Requirements

- **Type Safety**: Full TypeScript with strict mode
- **Error Handling**: Never swallow errors silently
- **Logging**: Use structured logging (console for now)
- **Testing**: Unit tests for pure functions
- **No Mocks**: Test real behavior, not mocked interfaces

## File Naming Conventions

- kebab-case for files: `run-detail.ts`
- PascalCase for components: `RunDetail`
- camelCase for functions: `getRunDetails`

## Dependencies

```json
{
  "dependencies": {
    "electrobun": "latest"
  },
  "devDependencies": {
    "@types/bun": "latest",
    "typescript": "^5.0.0"
  }
}
```
