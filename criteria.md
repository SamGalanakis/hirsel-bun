# Hirsel Rewrite Evaluation Criteria

## Overview

This document defines evaluation criteria for the hirsel rewrite. Each phase has specific pass/fail criteria.

---

## Phase 1: Core Infrastructure

### 1.1 Project Setup
- [ ] `bunx electrobun init` creates project structure
- [ ] Project compiles with `bun build`
- [ ] Basic BrowserWindow opens on `bun run dev`

### 1.2 State Management
- [ ] SQLite database initializes with schema from `spec.md`
- [ ] State class can create/read/update all tables
- [ ] Migrations run without error on existing databases
- [ ] `bun test` passes for state operations

**Test command:**
```bash
bun test src/core/state.test.ts
```

### 1.3 Configuration
- [ ] Config loads from `~/.hirsel/config.toml`
- [ ] Environment variables override config
- [ ] Agent presets work (claude, gemini, etc.)

### 1.4 Git Utilities
- [ ] `getRepoRoot()` returns correct path
- [ ] `createWorktree()` creates worktree at expected location
- [ ] `listUnmergedBranches()` returns correct branches
- [ ] `getDiff()` returns valid diff output

---

## Phase 2: CLI Commands

### 2.1 Command Routing
- [ ] `hirsel --help` shows all commands
- [ ] `hirsel --version` shows version
- [ ] Unknown commands show helpful error

### 2.2 Run Management Commands

**hirsel go**
```bash
# Test: Create a new run
hirsel go test-run "Add a hello world function"
# Expect: Run created, workers spawned, status shows working
hirsel view test-run
# Expect: Status, workers, tasks visible
```

**hirsel view**
```bash
hirsel view test-run
# Expect: Formatted output showing run status, workers, tasks
```

**hirsel log**
```bash
hirsel log test-run
# Expect: Activity history displayed
hirsel log test-run -f
# Expect: Live tail of activity
```

**hirsel pause/resume**
```bash
hirsel pause test-run
# Expect: Workers paused, status changes to paused
hirsel resume test-run
# Expect: Workers resume, status changes to working
```

**hirsel runs**
```bash
hirsel runs
# Expect: Table of all runs with status
hirsel runs --json
# Expect: JSON array output
```

**hirsel delete**
```bash
hirsel delete test-run
# Expect: Run directory removed
```

### 2.3 Task Commands
```bash
hirsel task-add test-run my_task "Do something"
# Expect: Task added to run
hirsel tasks test-run
# Expect: Task list includes my_task
hirsel task-done test-run my_task
# Expect: Task marked done
hirsel task-reopen test-run my_task
# Expect: Task reopened
hirsel task-delete test-run my_task
# Expect: Task removed
```

### 2.4 Config Command
```bash
hirsel config
# Expect: Interactive agent selector appears
hirsel config claude
# Expect: Agent set to claude, config file updated
```

---

## Phase 3: Worker System

### 3.1 ACP Client
- [ ] Connects to ACP-compatible agents
- [ ] Sends prompts correctly
- [ ] Receives responses
- [ ] Handles tool calls (MCP)

### 3.2 Worker Spawning
- [ ] Workers spawn with correct environment
- [ ] Worker PID tracked in database
- [ ] Worker logs written to run directory
- [ ] Session ID captured for metrics

### 3.3 Worker Lifecycle
- [ ] Workers auto-scale based on worker_scale setting
- [ ] Workers pause when run paused
- [ ] Workers resume when run resumed
- [ ] Stale workers detected (pid not alive)

**Test scenario:**
```bash
hirsel go scale-test "Test scaling" --workers 3
# Expect: 3 workers spawned
hirsel view scale-test
# Expect: 3 workers shown, all working
hirsel pause scale-test
# Expect: All workers paused
hirsel resume scale-test
# Expect: All workers resume
```

### 3.4 Eval System
- [ ] Evals trigger on task completion
- [ ] Eval status tracked (running, passed, failed)
- [ ] Eval feedback stored
- [ ] Failed evals trigger rework

---

## Phase 4: Desktop App - Foundation

### 4.1 Window Creation
- [ ] App launches without error
- [ ] Window has correct title "Hirsel"
- [ ] Window remembers size/position
- [ ] Window controls work (minimize, maximize, close)

### 4.2 State Bridge
- [ ] RPC connection established between main and webview
- [ ] State updates sent to webview
- [ ] Commands from webview execute in main process

### 4.3 Basic Rendering
- [ ] App shows "No runs" when empty
- [ ] Runs list populates when runs exist
- [ ] Selecting run shows detail view

---

## Phase 5: Desktop App - Features

### 5.1 Run List Panel
- [ ] All runs displayed with correct status icons
- [ ] Progress shown (done/total tasks)
- [ ] Elapsed time displayed
- [ ] Click selects run
- [ ] Context menu works

### 5.2 Run Detail View
- [ ] Workers section shows all workers
- [ ] Leader badge (★) shown correctly
- [ ] Context utilization percentage displayed
- [ ] Tasks section shows all tasks with status
- [ ] Activity log scrolls and updates

### 5.3 Real-time Updates
- [ ] New runs appear automatically
- [ ] Status changes reflect immediately
- [ ] Task progress updates live
- [ ] Worker status updates live

### 5.4 Chat Panel
- [ ] Opens with keyboard shortcut or button
- [ ] Shows thread list
- [ ] Message history renders
- [ ] Can send messages
- [ ] Unread indicators work

### 5.5 Keyboard Navigation
| Key | Expected Behavior |
|-----|------------------|
| `j` | Move down in run list |
| `k` | Move up in run list |
| `Enter` | Select/expand run |
| `a` | Attach to worker |
| `p` | Pause run |
| `r` | Resume run |
| `c` | Toggle chat panel |
| `?` | Show help overlay |
| `q` | Quit app |

---

## Phase 6: Polish

### 6.1 Theming
- [ ] Consistent color scheme
- [ ] Status colors match reference (accent color for active)
- [ ] Dark mode appropriate

### 6.2 Error Handling
- [ ] Invalid run name shows error
- [ ] Network errors handled gracefully
- [ ] Git errors reported clearly

### 6.3 Performance
- [ ] App starts in <500ms
- [ ] UI updates don't cause jank
- [ ] Memory usage stable over time

---

## Integration Test Scenarios

### Scenario 1: Full Run Lifecycle
```bash
# 1. Start a run
hirsel go lifecycle-test "Add a function that returns 42"

# 2. Check view
hirsel view lifecycle-test
# Expect: Status working, tasks visible

# 3. Wait for completion or pause
hirsel pause lifecycle-test

# 4. Check diff
hirsel diff lifecycle-test
# Expect: Shows code changes

# 5. Resume if needed
hirsel resume lifecycle-test

# 6. Deliver
hirsel deliver lifecycle-test
# Expect: Branch created

# 7. Cleanup
hirsel delete lifecycle-test
```

### Scenario 2: Multi-Worker Coordination
```bash
hirsel go multi-test "Build a calculator with add, subtract, multiply, divide" --workers 4
# Expect: 4 workers spawn
# Expect: Tasks distributed among workers
# Expect: Workers merge to staging as they complete
```

### Scenario 3: Desktop App Usage
1. Launch `hirsel` (opens desktop app)
2. See existing runs in left panel
3. Click run to see details
4. Watch workers progress in real-time
5. Use `p` to pause, `r` to resume
6. Open chat with `c`, send message
7. Quit with `q`

---

## Acceptance Criteria

### CLI Complete When:
1. All commands from `hirsel --help` functional
2. Feature parity with reference implementation
3. Shell completions work for bash/zsh
4. `--json` output available where documented

### Desktop App Complete When:
1. All panels render correctly
2. Real-time updates work
3. All keyboard shortcuts functional
4. Can manage runs without touching CLI

### Project Complete When:
1. CLI passes all integration tests
2. Desktop app passes all feature tests
3. No regressions from reference implementation
4. Documentation complete
