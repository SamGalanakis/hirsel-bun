# SpecFlow Implementation Plan

> **HISTORICAL:** This planning doc references the old Alpine.js/TypeScript frontend.
> The implementation has been migrated to SolidJS in `src/components/specflow/`.

**Status: Complete** - Jan 2026

Current implementation:
- Backend: `src-tauri/src/core/specflow/` (types.rs, state.rs, mod.rs)
- Commands: `src-tauri/src/gui/commands/specflow.rs`
- Frontend: `src/components/specflow/` (SpecflowBoard.tsx, NodeRenderer.tsx, etc.)

## Overview

SpecFlow is a 2D infinite canvas where project features are laid out as "Islands." Each island contains a Trifecta Grid (Spec | Tasks | Eval) representing the intent, reality, and proof of a feature. The board is the default view when a project is selected (not a run).

**Key Adaptations from Original Spec:**
- JSON → SQLite (project-level database)
- ProseMirror → Textarea + Markdown (consistency with existing draft editor)
- Fits into existing 3-panel layout (sidebar, center, AI chat)
- Integrates with existing run dispatch system

---

## 1. Data Model (Rust/SQLite)

### 1.1 Project Database Schema

Each project gets a `specflow.db` alongside the existing project config. Location: `~/.hirsel/projects/{id}/specflow.db`

```sql
-- Islands are feature containers on the canvas
CREATE TABLE islands (
    id TEXT PRIMARY KEY,              -- UUID
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',  -- draft, ready, dispatched, done, failed
    x REAL NOT NULL DEFAULT 0,        -- World position
    y REAL NOT NULL DEFAULT 0,
    width REAL DEFAULT 400,
    collapsed INTEGER DEFAULT 0,       -- LOAD override: user collapsed
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    run_name TEXT,                     -- Associated run (if dispatched)
    summary TEXT                       -- AI-generated summary for LOAD
);

-- Rows within an island's Trifecta Grid
CREATE TABLE rows (
    id TEXT PRIMARY KEY,              -- UUID
    island_id TEXT NOT NULL REFERENCES islands(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,         -- Order within island

    -- Spec column (Intent)
    spec_content TEXT,                 -- Markdown
    spec_status TEXT DEFAULT 'draft',  -- draft, approved

    -- Task column (Reality)
    task_title TEXT,
    task_description TEXT,             -- Markdown
    task_status TEXT DEFAULT 'todo',   -- todo, doing, done, blocked, deleted
    task_worker TEXT,                  -- Claimed by worker
    task_blocked_by TEXT,              -- JSON array of row IDs this task depends on

    -- Eval column (Proof)
    eval_criterion TEXT,               -- Markdown
    eval_status TEXT DEFAULT 'pending', -- pending, pass, fail
    eval_result TEXT,

    -- Dispatch tracking (row-level, not island-level)
    dispatched INTEGER DEFAULT 0,      -- Whether this row is in a run
    run_name TEXT,                     -- Associated run name

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Dependency wires between islands
CREATE TABLE wires (
    id TEXT PRIMARY KEY,
    from_island_id TEXT NOT NULL REFERENCES islands(id) ON DELETE CASCADE,
    to_island_id TEXT NOT NULL REFERENCES islands(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE(from_island_id, to_island_id)
);

-- Saved viewport positions (bookmarks)
CREATE TABLE bookmarks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    zoom REAL NOT NULL,
    created_at TEXT NOT NULL
);

-- Undo/redo history for global actions
CREATE TABLE action_history (
    id INTEGER PRIMARY KEY,
    action_type TEXT NOT NULL,         -- create_island, move_island, delete_island, etc.
    action_data TEXT NOT NULL,         -- JSON payload
    timestamp TEXT NOT NULL,
    undone INTEGER DEFAULT 0
);

CREATE INDEX idx_rows_island ON rows(island_id);
CREATE INDEX idx_wires_from ON wires(from_island_id);
CREATE INDEX idx_wires_to ON wires(to_island_id);
```

### 1.2 Rust Types

```rust
// src-tauri/src/core/specflow/types.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Island {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub collapsed: bool,
    pub created_at: String,
    pub updated_at: String,
    pub summary: Option<String>,
    pub rows: Vec<Row>,
}

// Island status is computed from row statuses:
// - "draft" if no rows dispatched
// - "partial" if some rows dispatched
// - "dispatched" if all rows dispatched
// - "done" if all dispatched rows' tasks are done
impl Island {
    pub fn computed_status(&self) -> &'static str {
        let dispatched_rows: Vec<_> = self.rows.iter().filter(|r| r.dispatched).collect();
        if dispatched_rows.is_empty() {
            return "draft";
        }
        if dispatched_rows.len() < self.rows.len() {
            return "partial";
        }
        if dispatched_rows.iter().all(|r| r.task_status == TaskStatus::Done) {
            return "done";
        }
        "dispatched"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub island_id: String,
    pub position: i32,
    // Spec
    pub spec_content: Option<String>,
    pub spec_status: SpecStatus,
    // Task
    pub task_title: Option<String>,
    pub task_description: Option<String>,
    pub task_status: TaskStatus,  // todo, doing, done, blocked, deleted
    pub task_worker: Option<String>,
    pub task_blocked_by: Vec<String>,  // Row IDs this task depends on
    // Eval
    pub eval_criterion: Option<String>,
    pub eval_status: EvalStatus,
    pub eval_result: Option<String>,
    // Dispatch tracking
    pub dispatched: bool,
    pub run_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
    Blocked,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wire {
    pub id: String,
    pub from_island_id: String,
    pub to_island_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeDiff {
    pub file_path: String,
    pub old_content: String,
    pub new_content: String,
}
```

---

## 2. Backend Implementation

### 2.1 New Module: `src-tauri/src/core/specflow/`

```
specflow/
├── mod.rs          -- Module exports, SpecFlowState struct
├── types.rs        -- Island, Row, Wire, Bookmark types
├── state.rs        -- SQLite operations
└── ops.rs          -- Higher-level operations (dispatch, sync with runs)
```

### 2.2 Tauri Commands

Add to `src-tauri/src/gui/commands/specflow.rs`:

| Command | Args | Returns | Purpose |
|---------|------|---------|---------|
| `get_project_islands` | `projectId` | `Vec<Island>` | Load all islands with rows |
| `create_island` | `projectId, name, x, y` | `Island` | Create new island |
| `update_island` | `projectId, islandId, updates` | `Island` | Update name, position, status |
| `delete_island` | `projectId, islandId` | `()` | Remove island and rows |
| `create_row` | `projectId, islandId, position` | `Row` | Add row to island |
| `update_row` | `projectId, rowId, updates` | `Row` | Update any row field |
| `delete_row` | `projectId, rowId` | `()` | Remove row |
| `reorder_rows` | `projectId, islandId, rowIds` | `()` | Reorder rows |
| `create_wire` | `projectId, fromId, toId` | `Wire` | Add dependency |
| `delete_wire` | `projectId, wireId` | `()` | Remove dependency |
| `get_wires` | `projectId` | `Vec<Wire>` | Get all wires |
| `dispatch_rows` | `projectId, rowIds` | `DispatchResult` | Create run from selected rows |
| `dispatch_rows_confirm` | `projectId, rowIds` | `RunDetail` | Dispatch after warning acknowledged |
| `sync_run_status` | `projectId, runName` | `()` | Update rows from run |
| `set_task_blocked_by` | `projectId, rowId, blockedByIds` | `()` | Set task dependencies |
| `get_bookmarks` | `projectId` | `Vec<Bookmark>` | Get saved views |
| `save_bookmark` | `projectId, name, x, y, zoom` | `Bookmark` | Save viewport |
| `delete_bookmark` | `projectId, bookmarkId` | `()` | Remove bookmark |
| `generate_island_summary` | `projectId, islandId` | `String` | AI summary for LOAD |

### 2.3 Run Dispatch Integration

**Selection is at ROW level, not island level.** Users can select arbitrary rows from any island. Dependencies are auto-selected based on task `blocked_by` relationships.

When dispatching rows to a run:

1. Collect selected rows + their `blocked_by` dependencies (ripple selection)
2. **Warn if rows already dispatched**: If any selected row is already in an active run, show warning before proceeding
3. **Generate `spec.md`**: Concatenate spec columns from selected rows, grouped by island, in board Y-order
4. **Generate `eval.md`**: Same for eval columns - concatenate in board order
5. **Generate initial tasks**: Extract task titles/descriptions from selected rows
6. Create **draft** run via existing `create_draft` flow (user can still edit before starting)
7. **Pass initial tasks to draft**: New `initial_tasks` field on draft (see below)
8. Update row status to `dispatched`, store `run_name` on each row
9. Set up polling to sync run task status back to rows

**Initial Tasks Flow:**

The run still starts with a "scope" task as the root, but initial tasks from the board are pre-populated:

```
Scope Task (root)
├── [Initial Task 1 from board] - status: todo
├── [Initial Task 2 from board] - status: todo
└── [Initial Task 3 from board] - status: todo
```

The worker prompt is updated to inform the model:

```markdown
## Initial Tasks

The following tasks have been identified during planning. You should:
1. Review these tasks after exploring the codebase
2. Extend, modify, or add to them as needed based on your findings
3. You are not bound to these exact tasks - they are starting points

Tasks:
- [ ] Task 1: Description...
- [ ] Task 2: Description...
```

This preserves the agent's autonomy while giving it a structured starting point from the board.

**Spec Generation Example:**

Given islands selected in this order (by Y position on board):
- "Authentication" (y: 100)
- "User Profile" (y: 300)
- "Settings" (y: 500)

Generated `spec.md`:
```markdown
# Authentication

[spec content from Authentication island rows]

# User Profile

[spec content from User Profile island rows]

# Settings

[spec content from Settings island rows]
```

```rust
// In specflow/ops.rs
pub async fn dispatch_rows(
    project_id: i64,
    row_ids: Vec<String>,
    config: &Config,
) -> Result<DispatchResult> {
    let specflow_state = SpecFlowState::open(project_id)?;

    // Get selected rows and expand with blocked_by dependencies
    let mut selected_rows = specflow_state.get_rows_with_deps(&row_ids)?;

    // Check for already-dispatched rows and warn
    let already_dispatched: Vec<_> = selected_rows.iter()
        .filter(|r| r.dispatched)
        .map(|r| r.id.clone())
        .collect();

    if !already_dispatched.is_empty() {
        return Ok(DispatchResult::Warning {
            message: format!("{} rows are already in active runs", already_dispatched.len()),
            row_ids: already_dispatched,
        });
    }

    // Sort rows by island Y position, then row position within island
    selected_rows.sort_by(|a, b| {
        let island_a = specflow_state.get_island(&a.island_id).unwrap();
        let island_b = specflow_state.get_island(&b.island_id).unwrap();
        island_a.y.partial_cmp(&island_b.y)
            .unwrap()
            .then(a.position.cmp(&b.position))
    });

    // Group rows by island for spec generation
    let rows_by_island = group_rows_by_island(&selected_rows, &specflow_state)?;

    // Generate spec.md (concatenated by board order)
    let spec = generate_spec_from_rows(&rows_by_island);

    // Generate eval.md (concatenated by board order)
    let eval = generate_eval_from_rows(&rows_by_island);

    // Extract initial tasks
    let initial_tasks: Vec<InitialTask> = selected_rows.iter()
        .filter(|row| row.task_title.is_some())
        .map(|row| {
            let island = specflow_state.get_island(&row.island_id).unwrap();
            InitialTask {
                title: row.task_title.clone().unwrap(),
                description: row.task_description.clone(),
                source_island: island.name.clone(),
                source_row_id: row.id.clone(),
                blocked_by: row.task_blocked_by.clone(),  // Preserve dependencies
            }
        })
        .collect();

    // Create draft run
    let run_name = generate_run_name_from_rows(&rows_by_island);
    let run = ops::create_draft(CreateDraftRequest {
        name: run_name.clone(),
        spec,
        eval: Some(eval),
        project_id: Some(project_id),
        initial_tasks: Some(initial_tasks),
        // ... other fields from project config
    }).await?;

    // Mark rows as dispatched
    for row in &selected_rows {
        specflow_state.set_row_dispatched(&row.id, &run_name)?;
    }

    Ok(DispatchResult::Success { run })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialTask {
    pub title: String,
    pub description: Option<String>,
    pub source_island: String,
    pub source_row_id: String,
    pub blocked_by: Vec<String>,  // Row IDs (mapped to task IDs in run)
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DispatchResult {
    Success { run: RunDetail },
    Warning { message: String, row_ids: Vec<String> },
}
```

### 2.4 Changes to Existing Run System

**Modify `CreateDraftRequest`** in `src-tauri/src/core/ops/types.rs`:

```rust
pub struct CreateDraftRequest {
    // ... existing fields ...
    pub initial_tasks: Option<Vec<InitialTask>>,
}
```

**Modify `start_draft`** in `src-tauri/src/core/ops/run.rs`:

When starting a run with initial tasks:
1. Create the scope task as usual
2. Insert initial tasks as children of scope with `status: Todo`
3. Store `source_row_id` in task metadata for back-sync

**Modify worker prompt** in `src-tauri/src/worker/acp_client.rs`:

When initial tasks exist, add to the prompt:

```markdown
## Initial Tasks

The following tasks were identified during planning. Review them after
exploring the codebase - you may extend, modify, or add new tasks as needed.

{{#each initial_tasks}}
- [ ] {{title}}{{#if description}}: {{description}}{{/if}}
{{/each}}
```

**Task back-sync** in `src-tauri/src/core/specflow/ops.rs`:

Poll run tasks and update board rows:
```rust
pub async fn sync_run_to_board(project_id: i64, run_name: &str) -> Result<()> {
    let run_state = RunState::open(run_name)?;
    let specflow_state = SpecFlowState::open(project_id)?;

    for task in run_state.get_tasks()? {
        if let Some(source_row_id) = task.metadata.get("source_row_id") {
            specflow_state.update_row_task_status(
                source_row_id,
                task.status.into(),
                task.claimed_by.clone(),
            )?;
        }
    }
    Ok(())
}
```

**Task Status Mapping:**

| Run Task Status | Board Row Display | Notes |
|-----------------|-------------------|-------|
| `todo` | ⚪ Queued | Initial state |
| `doing` | 🟡 In Progress | Shows worker name |
| `done` | 🟢 Completed | |
| `blocked` | 🔵 Blocked | Waiting on dependencies |
| `deleted` | ⚫ Deleted | Strikethrough, worker decided not needed |

Workers can delete initial tasks if they determine they're not needed after exploration. They cannot rename or split tasks - instead they add child tasks. This keeps sync simple: the original row always exists on the board.

### 2.5 Review Mode (Simplified)

When a run completes, dispatched rows show a "review card" instead of raw status:

```
┌─────────────────────────────────────┐
│ ✓ Run: feature-auth-123             │
│   Status: Done (3/3 tasks)          │
│   [View Run] [Deliver] [Dismiss]    │
└─────────────────────────────────────┘
```

- **View Run**: Switches to run detail view
- **Deliver**: Triggers `deliver_run` (pushes to branch)
- **Dismiss**: Clears the run association, resets row to draft

No diff view in V1. Full review mode with inline diffs can be added in V2.
```

---

## 3. Frontend Implementation

### 3.1 File Structure

```
src/lib/components/specflow-board/
├── index.ts              -- Main Alpine component (specflowBoard)
├── types.ts              -- Frontend TypeScript types
├── canvas-renderer.ts    -- Grid + wire drawing
├── viewport.ts           -- Viewport math, culling, LOAD
└── cmd-k.ts              -- Jump-to palette logic

src/templates/
└── specflow-board.html   -- Board template

src/lib/
├── types.ts              -- Add Island, Wire, etc.
└── api.ts                -- Add specflow API functions
```

### 3.2 View Switching Logic

Update `src/index.html` to add SpecFlow board view:

```html
<!-- Center panel view switching -->
<div x-show="selectedProject && !selectedRun" class="flex-1">
  <!-- SpecFlow Board -->
  <div x-data="specflowBoard()" x-init="init()" @destroy="destroy()"
       class="w-full h-full">
    <!-- include specflow-board.html -->
  </div>
</div>

<div x-show="selectedRun && currentRunDetail?.status === 'draft'" class="flex-1">
  <!-- Draft Editor (existing) -->
</div>

<div x-show="selectedRun && currentRunDetail?.status !== 'draft'" class="flex-1">
  <!-- Run Details (existing) -->
</div>
```

Update `appState()` to add project selection:

```typescript
// In app-state.ts
selectedProject: null as number | null,
selectedProjectName: null as string | null,

selectProject(id: number, name: string) {
  this.selectedProject = id;
  this.selectedProjectName = name;
  this.selectedRun = null;  // Clear run selection
  window.dispatchEvent(new CustomEvent('project-selected', { detail: { id, name } }));
},

clearProject() {
  this.selectedProject = null;
  this.selectedProjectName = null;
  window.dispatchEvent(new CustomEvent('project-cleared'));
},
```

### 3.3 Left Sidebar Updates

Add project list section above runs:

```html
<!-- In run-list-panel.html -->
<div class="border-b border-pasture-600 pb-2 mb-2">
  <div class="px-3 py-2 text-xs font-semibold text-wool-400 uppercase">Projects</div>
  <template x-for="project in projects" :key="project.id">
    <div @click="selectProject(project.id, project.name)"
         :class="selectedProject === project.id ? 'bg-amber-500/20' : 'hover:bg-pasture-700'"
         class="px-3 py-2 cursor-pointer flex items-center gap-2">
      <i data-lucide="layout-grid" class="w-4 h-4 text-wool-400"></i>
      <span class="text-sm text-wool-200" x-text="project.name"></span>
    </div>
  </template>
  <button @click="createProject()" class="w-full px-3 py-2 text-left text-sm text-wool-400 hover:bg-pasture-700">
    <i data-lucide="plus" class="w-4 h-4 inline mr-2"></i> New Project
  </button>
</div>

<!-- Runs section (existing, filtered by project) -->
<div class="px-3 py-2 text-xs font-semibold text-wool-400 uppercase">
  <span x-text="selectedProject ? 'Project Runs' : 'All Runs'"></span>
</div>
```

### 3.4 SpecFlow Board Component

```typescript
// src/lib/components/specflow-board/index.ts

import { zoom, zoomIdentity, type ZoomBehavior, type D3ZoomEvent } from 'd3-zoom';
import { select } from 'd3-selection';
import { CanvasRenderer } from './canvas-renderer';
import type { Island, Wire, Bookmark } from './types';

export function specflowBoard() {
  return {
    // State
    projectId: null as number | null,
    islands: [] as Island[],
    visibleIslands: [] as Island[],
    wires: [] as Wire[],
    bookmarks: [] as Bookmark[],

    // Transform state
    transform: { x: 0, y: 0, k: 1 },
    load: 'near' as 'far' | 'mid' | 'near',

    // Selection state
    selectedIslandId: null as string | null,
    editingCell: null as { islandId: string; rowId: string; column: 'spec' | 'task' | 'eval' } | null,

    // Run mode (for dispatch selection at row level)
    runMode: false,

    // UI state
    cmdKOpen: false,

    // Internal
    _canvasRenderer: null as CanvasRenderer | null,
    _zoomBehavior: null as ZoomBehavior<HTMLDivElement, unknown> | null,
    _eventCleanups: [] as (() => void)[],
    _rafId: null as number | null,

    // Lifecycle
    async init() {
      // Listen for project selection
      const projectSelectedHandler = async (e: CustomEvent) => {
        this.projectId = e.detail.id;
        await this.loadProject();
      };
      window.addEventListener('project-selected', projectSelectedHandler as EventListener);
      this._eventCleanups.push(() =>
        window.removeEventListener('project-selected', projectSelectedHandler as EventListener)
      );

      // Initialize canvas renderer
      const canvas = this.$refs.gridCanvas as HTMLCanvasElement;
      this._canvasRenderer = new CanvasRenderer(canvas);

      // Initialize d3-zoom
      this.initZoom();

      // Handle resize
      const resizeObserver = new ResizeObserver(() => this.handleResize());
      resizeObserver.observe(this.$refs.boardContainer as HTMLElement);
      this._eventCleanups.push(() => resizeObserver.disconnect());

      // Keyboard shortcuts
      const keyHandler = (e: KeyboardEvent) => this.handleKeydown(e);
      document.addEventListener('keydown', keyHandler);
      this._eventCleanups.push(() => document.removeEventListener('keydown', keyHandler));
    },

    destroy() {
      this._eventCleanups.forEach(fn => fn());
      this._eventCleanups = [];
      if (this._rafId) cancelAnimationFrame(this._rafId);
    },

    // Data loading
    async loadProject() {
      if (!this.projectId) return;

      try {
        const [islands, wires, bookmarks] = await Promise.all([
          window.tauriInvoke<Island[]>('get_project_islands', { projectId: this.projectId }),
          window.tauriInvoke<Wire[]>('get_wires', { projectId: this.projectId }),
          window.tauriInvoke<Bookmark[]>('get_bookmarks', { projectId: this.projectId }),
        ]);

        this.islands = islands;
        this.wires = wires;
        this.bookmarks = bookmarks;
        this.updateVisibleIslands();
        this.render();
      } catch (e) {
        console.error('Failed to load project:', e);
        window.toast?.error('Failed to load project');
      }
    },

    // Zoom/pan
    initZoom() {
      const container = this.$refs.boardContainer as HTMLDivElement;

      this._zoomBehavior = zoom<HTMLDivElement, unknown>()
        .scaleExtent([0.1, 3])
        .filter((event: Event) => {
          const target = event.target as HTMLElement;
          // Don't capture zoom on interactive elements
          if (target.closest('.island-interactive')) return false;
          if (event.type === 'wheel') return true;
          const mouseEvent = event as MouseEvent;
          return mouseEvent.button === 1 || !target.closest('.island');
        })
        .on('zoom', (event: D3ZoomEvent<HTMLDivElement, unknown>) => {
          const { x, y, k } = event.transform;
          this.transform = { x, y, k };
          this.load = k < 0.4 ? 'far' : k < 0.8 ? 'mid' : 'near';
          this.scheduleRender();
        });

      select(container).call(this._zoomBehavior);
    },

    scheduleRender() {
      if (this._rafId) return;
      this._rafId = requestAnimationFrame(() => {
        this._rafId = null;
        this.updateVisibleIslands();
        this.render();
      });
    },

    updateVisibleIslands() {
      const container = this.$refs.boardContainer as HTMLElement;
      if (!container) return;

      const { x, y, k } = this.transform;
      const vw = container.clientWidth;
      const vh = container.clientHeight;

      // Convert viewport to world coordinates
      const worldLeft = -x / k;
      const worldTop = -y / k;
      const worldRight = worldLeft + vw / k;
      const worldBottom = worldTop + vh / k;

      // Buffer zone (200px world space)
      const buffer = 200;

      this.visibleIslands = this.islands.filter(island => {
        const iRight = island.x + island.width;
        const iBottom = island.y + 300; // Approximate height

        return !(iRight < worldLeft - buffer ||
                 island.x > worldRight + buffer ||
                 iBottom < worldTop - buffer ||
                 island.y > worldBottom + buffer);
      });
    },

    render() {
      if (!this._canvasRenderer) return;
      const container = this.$refs.boardContainer as HTMLElement;
      this._canvasRenderer.render(
        this.transform,
        this.wires,
        this.islands,
        container.clientWidth,
        container.clientHeight
      );
    },

    handleResize() {
      const canvas = this.$refs.gridCanvas as HTMLCanvasElement;
      const container = this.$refs.boardContainer as HTMLElement;

      canvas.width = container.clientWidth * devicePixelRatio;
      canvas.height = container.clientHeight * devicePixelRatio;
      canvas.style.width = `${container.clientWidth}px`;
      canvas.style.height = `${container.clientHeight}px`;

      this._canvasRenderer?.resize(container.clientWidth, container.clientHeight);
      this.render();
    },

    // Island interactions
    async createIsland(x: number, y: number) {
      if (!this.projectId) return;

      const island = await window.tauriInvoke<Island>('create_island', {
        projectId: this.projectId,
        name: 'New Feature',
        x: (x - this.transform.x) / this.transform.k,
        y: (y - this.transform.y) / this.transform.k,
      });

      this.islands.push(island);
      this.selectedIslandId = island.id;
      this.updateVisibleIslands();
      this.render();
    },

    startIslandDrag(islandId: string, event: PointerEvent) {
      event.stopPropagation();
      const island = this.islands.find(i => i.id === islandId);
      if (!island) return;

      const startX = event.clientX;
      const startY = event.clientY;
      const origX = island.x;
      const origY = island.y;

      const onMove = (e: PointerEvent) => {
        const dx = (e.clientX - startX) / this.transform.k;
        const dy = (e.clientY - startY) / this.transform.k;
        island.x = origX + dx;
        island.y = origY + dy;
        this.render();
      };

      const onUp = async () => {
        window.removeEventListener('pointermove', onMove);
        window.removeEventListener('pointerup', onUp);

        await window.tauriInvoke('update_island', {
          projectId: this.projectId,
          islandId: island.id,
          updates: { x: island.x, y: island.y }
        });
      };

      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
    },

    // Cell editing (Hollow Block pattern)
    startEditing(islandId: string, rowId: string, column: 'spec' | 'task' | 'eval') {
      this.editingCell = { islandId, rowId, column };
      // Focus textarea after Alpine renders it
      this.$nextTick(() => {
        const textarea = this.$refs[`editor-${rowId}-${column}`] as HTMLTextAreaElement;
        textarea?.focus();
      });
    },

    async saveEdit() {
      if (!this.editingCell || !this.projectId) return;

      const { islandId, rowId, column } = this.editingCell;
      const textarea = this.$refs[`editor-${rowId}-${column}`] as HTMLTextAreaElement;
      const content = textarea?.value || '';

      const updates: Record<string, string> = {};
      if (column === 'spec') updates.spec_content = content;
      else if (column === 'task') updates.task_description = content;
      else if (column === 'eval') updates.eval_criterion = content;

      await window.tauriInvoke('update_row', {
        projectId: this.projectId,
        rowId,
        updates
      });

      // Update local state
      const island = this.islands.find(i => i.id === islandId);
      const row = island?.rows.find(r => r.id === rowId);
      if (row) Object.assign(row, updates);

      this.editingCell = null;
    },

    // Run dispatch (row-level selection)
    selectedRowsForDispatch: new Set<string>(),
    dispatchWarning: null as { message: string; rowIds: string[] } | null,

    toggleRunMode() {
      this.runMode = !this.runMode;
      if (!this.runMode) {
        this.selectedRowsForDispatch.clear();
        this.dispatchWarning = null;
      }
    },

    toggleRowForDispatch(rowId: string) {
      if (this.selectedRowsForDispatch.has(rowId)) {
        this.selectedRowsForDispatch.delete(rowId);
      } else {
        this.selectedRowsForDispatch.add(rowId);
        // Ripple selection: add blocked_by dependencies
        this.addRowDependenciesToSelection(rowId);
      }
    },

    addRowDependenciesToSelection(rowId: string) {
      // Find the row and add its blocked_by dependencies
      for (const island of this.islands) {
        const row = island.rows.find(r => r.id === rowId);
        if (row) {
          for (const depId of row.task_blocked_by || []) {
            if (!this.selectedRowsForDispatch.has(depId)) {
              this.selectedRowsForDispatch.add(depId);
              this.addRowDependenciesToSelection(depId); // Recursive
            }
          }
          break;
        }
      }
    },

    async dispatchSelectedRows() {
      if (!this.projectId || this.selectedRowsForDispatch.size === 0) return;

      try {
        const result = await window.tauriInvoke<{
          Success?: { run: { name: string } };
          Warning?: { message: string; row_ids: string[] };
        }>('dispatch_rows', {
          projectId: this.projectId,
          rowIds: Array.from(this.selectedRowsForDispatch)
        });

        if (result.Warning) {
          // Show warning, let user confirm
          this.dispatchWarning = {
            message: result.Warning.message,
            rowIds: result.Warning.row_ids
          };
          return;
        }

        if (result.Success) {
          window.toast?.success(`Created draft: ${result.Success.run.name}`);
          this.runMode = false;
          this.selectedRowsForDispatch.clear();

          // Refresh to show updated statuses
          await this.loadProject();

          // Select the new draft run for editing
          window.dispatchEvent(new CustomEvent('run-selected', { detail: result.Success.run.name }));
        }
      } catch (e) {
        console.error('Failed to dispatch:', e);
        window.toast?.error('Failed to create run');
      }
    },

    async confirmDispatchWithWarning() {
      if (!this.projectId) return;

      try {
        const run = await window.tauriInvoke<{ name: string }>('dispatch_rows_confirm', {
          projectId: this.projectId,
          rowIds: Array.from(this.selectedRowsForDispatch)
        });

        window.toast?.success(`Created draft: ${run.name}`);
        this.runMode = false;
        this.selectedRowsForDispatch.clear();
        this.dispatchWarning = null;

        await this.loadProject();
        window.dispatchEvent(new CustomEvent('run-selected', { detail: run.name }));
      } catch (e) {
        console.error('Failed to dispatch:', e);
        window.toast?.error('Failed to create run');
      }
    },

    cancelDispatchWarning() {
      this.dispatchWarning = null;
    },

    // Keyboard shortcuts
    handleKeydown(e: KeyboardEvent) {
      // Cmd+K: Jump to
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        this.cmdKOpen = true;
        return;
      }

      // R: Toggle run mode
      if (e.key === 'r' && !this.editingCell && document.activeElement?.tagName !== 'INPUT') {
        this.toggleRunMode();
        return;
      }

      // Escape: Close editing, clear selection
      if (e.key === 'Escape') {
        if (this.editingCell) {
          this.editingCell = null;
        } else if (this.cmdKOpen) {
          this.cmdKOpen = false;
        } else if (this.runMode) {
          this.runMode = false;
          this.selectedForDispatch.clear();
        }
      }
    },

    // Navigation
    jumpToIsland(islandId: string) {
      const island = this.islands.find(i => i.id === islandId);
      if (!island || !this._zoomBehavior) return;

      const container = this.$refs.boardContainer as HTMLElement;
      const centerX = container.clientWidth / 2;
      const centerY = container.clientHeight / 2;

      // Animate to center the island
      select(container)
        .transition()
        .duration(500)
        .call(
          this._zoomBehavior.transform,
          zoomIdentity
            .translate(centerX, centerY)
            .scale(1)
            .translate(-island.x - island.width / 2, -island.y - 150)
        );

      this.cmdKOpen = false;
      this.selectedIslandId = islandId;
    },

    fitAll() {
      if (this.islands.length === 0 || !this._zoomBehavior) return;

      // Calculate bounding box
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (const island of this.islands) {
        minX = Math.min(minX, island.x);
        minY = Math.min(minY, island.y);
        maxX = Math.max(maxX, island.x + island.width);
        maxY = Math.max(maxY, island.y + 300);
      }

      const container = this.$refs.boardContainer as HTMLElement;
      const padding = 50;
      const scaleX = (container.clientWidth - padding * 2) / (maxX - minX);
      const scaleY = (container.clientHeight - padding * 2) / (maxY - minY);
      const scale = Math.min(scaleX, scaleY, 1);

      const centerX = (minX + maxX) / 2;
      const centerY = (minY + maxY) / 2;

      select(container)
        .transition()
        .duration(500)
        .call(
          this._zoomBehavior.transform,
          zoomIdentity
            .translate(container.clientWidth / 2, container.clientHeight / 2)
            .scale(scale)
            .translate(-centerX, -centerY)
        );
    },

    // Helpers for templates
    getIslandStyle(island: Island) {
      return `left: ${island.x}px; top: ${island.y}px; width: ${island.width}px;`;
    },

    getIslandClass(island: Island) {
      const classes = ['island', 'absolute', 'rounded-lg', 'border', 'bg-pasture-800', 'shadow-lg', 'overflow-hidden'];

      if (this.selectedIslandId === island.id) {
        classes.push('ring-2', 'ring-amber-500');
      }

      // In run mode, highlight islands that have selected rows
      if (this.runMode) {
        const hasSelectedRows = island.rows?.some(r => this.selectedRowsForDispatch.has(r.id));
        if (hasSelectedRows) {
          classes.push('border-amber-500');
        } else {
          classes.push('border-pasture-600', 'opacity-60');
        }
      } else {
        classes.push('border-pasture-600');
      }

      return classes.join(' ');
    },

    getStatusDotClass(status: string) {
      switch (status) {
        case 'draft': return 'bg-wool-500';
        case 'partial': return 'bg-amber-500/50';
        case 'dispatched': return 'bg-amber-500 animate-pulse';
        case 'done': return 'bg-sage-500';
        default: return 'bg-wool-500';
      }
    },

    getTaskStatusDotClass(status: string) {
      switch (status) {
        case 'todo': return 'bg-wool-500';
        case 'doing': return 'bg-amber-500 animate-pulse';
        case 'done': return 'bg-sage-500';
        case 'blocked': return 'bg-sky-500';
        case 'deleted': return 'bg-wool-700';
        default: return 'bg-wool-500';
      }
    },

    selectRun(runName: string) {
      window.dispatchEvent(new CustomEvent('run-selected', { detail: runName }));
    },

    async deliverRun(runName: string) {
      try {
        await window.tauriInvoke('deliver_run', { runName });
        window.toast?.success(`Delivered: ${runName}`);
        await this.loadProject();
      } catch (e) {
        console.error('Failed to deliver:', e);
        window.toast?.error('Failed to deliver run');
      }
    },
  };
}
```

### 3.5 Canvas Renderer

```typescript
// src/lib/components/specflow-board/canvas-renderer.ts

export class CanvasRenderer {
  private ctx: CanvasRenderingContext2D;
  private dpr: number;
  private width: number = 0;
  private height: number = 0;

  constructor(canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext('2d')!;
    this.dpr = window.devicePixelRatio || 1;
  }

  resize(width: number, height: number) {
    this.width = width;
    this.height = height;
  }

  render(
    transform: { x: number; y: number; k: number },
    wires: Array<{ from_island_id: string; to_island_id: string }>,
    islands: Array<{ id: string; x: number; y: number; width: number }>,
    viewportWidth: number,
    viewportHeight: number
  ) {
    const { ctx, dpr } = this;
    const { x, y, k } = transform;

    // Clear
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, viewportWidth * dpr, viewportHeight * dpr);

    // Apply transform
    ctx.setTransform(dpr * k, 0, 0, dpr * k, dpr * x, dpr * y);

    // Draw grid
    this.drawGrid(transform, viewportWidth, viewportHeight);

    // Draw wires
    this.drawWires(wires, islands);
  }

  private drawGrid(
    transform: { x: number; y: number; k: number },
    vw: number,
    vh: number
  ) {
    const { ctx } = this;
    const { x, y, k } = transform;

    // Grid spacing based on zoom
    const majorSpacing = 100;
    const minorSpacing = 20;
    const showMinor = k >= 0.3;

    // Calculate visible world bounds
    const worldLeft = Math.floor(-x / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldTop = Math.floor(-y / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldRight = Math.ceil((-x + vw) / k / majorSpacing) * majorSpacing + majorSpacing;
    const worldBottom = Math.ceil((-y + vh) / k / majorSpacing) * majorSpacing + majorSpacing;

    // Minor grid (dots)
    if (showMinor) {
      ctx.fillStyle = 'rgba(255, 255, 255, 0.05)';
      for (let wx = worldLeft; wx <= worldRight; wx += minorSpacing) {
        for (let wy = worldTop; wy <= worldBottom; wy += minorSpacing) {
          ctx.beginPath();
          ctx.arc(wx, wy, 1 / k, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    // Major grid (dots)
    ctx.fillStyle = 'rgba(255, 255, 255, 0.1)';
    for (let wx = worldLeft; wx <= worldRight; wx += majorSpacing) {
      for (let wy = worldTop; wy <= worldBottom; wy += majorSpacing) {
        ctx.beginPath();
        ctx.arc(wx, wy, 2 / k, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }

  private drawWires(
    wires: Array<{ from_island_id: string; to_island_id: string }>,
    islands: Array<{ id: string; x: number; y: number; width: number }>
  ) {
    const { ctx } = this;
    const islandMap = new Map(islands.map(i => [i.id, i]));

    ctx.strokeStyle = 'rgba(245, 158, 11, 0.5)'; // amber-500
    ctx.lineWidth = 2;

    for (const wire of wires) {
      const from = islandMap.get(wire.from_island_id);
      const to = islandMap.get(wire.to_island_id);
      if (!from || !to) continue;

      // Calculate connection points (right side of from, left side of to)
      const fromX = from.x + from.width;
      const fromY = from.y + 50; // Header height
      const toX = to.x;
      const toY = to.y + 50;

      // Draw bezier curve
      const cp1x = fromX + (toX - fromX) * 0.5;
      const cp1y = fromY;
      const cp2x = fromX + (toX - fromX) * 0.5;
      const cp2y = toY;

      ctx.beginPath();
      ctx.moveTo(fromX, fromY);
      ctx.bezierCurveTo(cp1x, cp1y, cp2x, cp2y, toX, toY);
      ctx.stroke();

      // Arrowhead
      const angle = Math.atan2(toY - cp2y, toX - cp2x);
      const arrowSize = 8;
      ctx.beginPath();
      ctx.moveTo(toX, toY);
      ctx.lineTo(
        toX - arrowSize * Math.cos(angle - Math.PI / 6),
        toY - arrowSize * Math.sin(angle - Math.PI / 6)
      );
      ctx.lineTo(
        toX - arrowSize * Math.cos(angle + Math.PI / 6),
        toY - arrowSize * Math.sin(angle + Math.PI / 6)
      );
      ctx.closePath();
      ctx.fillStyle = 'rgba(245, 158, 11, 0.5)';
      ctx.fill();
    }
  }
}
```

### 3.6 Board HTML Template

```html
<!-- src/templates/specflow-board.html -->
<div x-ref="boardContainer" class="relative w-full h-full overflow-hidden bg-pasture-950">
  <!-- Canvas layer (grid + wires) -->
  <canvas x-ref="gridCanvas" class="absolute inset-0 pointer-events-none"></canvas>

  <!-- Island container (transformed by zoom) -->
  <div x-ref="islandContainer"
       class="absolute inset-0 origin-top-left"
       :style="`transform: translate(${transform.x}px, ${transform.y}px) scale(${transform.k})`">

    <!-- Islands -->
    <template x-for="island in visibleIslands" :key="island.id">
      <div :class="getIslandClass(island)" :style="getIslandStyle(island)">

        <!-- Header (draggable) -->
        <div @pointerdown="startIslandDrag(island.id, $event)"
             @click="selectedIslandId = island.id"
             class="island-header px-3 py-2 border-b border-pasture-600 bg-pasture-700/50 cursor-grab flex items-center justify-between">
          <span class="text-sm font-medium text-wool-200 truncate" x-text="island.name"></span>
          <div class="flex items-center gap-2">
            <span class="w-2 h-2 rounded-full" :class="getStatusDotClass(island.computed_status())"></span>
          </div>
        </div>

        <!-- LOAD: Far view (zoom < 0.4) -->
        <div x-show="load === 'far'" class="p-2 text-center">
          <span class="text-xs text-wool-400" x-text="island.summary || `${island.rows?.length || 0} rows`"></span>
        </div>

        <!-- LOAD: Mid view (0.4 <= zoom < 0.8) -->
        <div x-show="load === 'mid'" class="p-2 space-y-1">
          <template x-for="row in island.rows?.slice(0, 3)" :key="row.id">
            <div class="text-xs text-wool-300 truncate" x-text="row.spec_content?.slice(0, 50) || 'Empty'"></div>
          </template>
          <template x-if="island.rows?.length > 3">
            <div class="text-xs text-wool-500">+<span x-text="island.rows.length - 3"></span> more</div>
          </template>
        </div>

        <!-- LOAD: Near view (zoom >= 0.8) - Full Trifecta Grid -->
        <div x-show="load === 'near'" class="island-interactive">
          <!-- Trifecta header -->
          <div class="grid grid-cols-3 text-xs font-semibold text-wool-400 border-b border-pasture-600">
            <div class="px-2 py-1 border-r border-pasture-600">Spec</div>
            <div class="px-2 py-1 border-r border-pasture-600">Tasks</div>
            <div class="px-2 py-1">Eval</div>
          </div>

          <!-- Rows -->
          <template x-for="row in island.rows" :key="row.id">
            <div class="grid border-b border-pasture-700 last:border-0"
                 :class="runMode ? 'grid-cols-[auto_1fr_1fr_1fr]' : 'grid-cols-3'"
                 :style="row.dispatched && row.run_name ? 'opacity: 0.7' : ''">

              <!-- Row selection checkbox (run mode only) -->
              <template x-if="runMode">
                <div class="p-2 border-r border-pasture-600 flex items-center justify-center">
                  <input type="checkbox"
                         :checked="selectedRowsForDispatch.has(row.id)"
                         @click.stop="toggleRowForDispatch(row.id)"
                         :disabled="row.dispatched"
                         class="w-4 h-4">
                </div>
              </template>

              <!-- Spec column -->
              <div class="p-2 border-r border-pasture-600 min-h-[60px]">
                <template x-if="editingCell?.rowId === row.id && editingCell?.column === 'spec'">
                  <textarea :x-ref="`editor-${row.id}-spec`"
                            x-text="row.spec_content"
                            @blur="saveEdit()"
                            @keydown.escape="editingCell = null"
                            class="w-full h-full bg-pasture-900 border border-amber-500 rounded p-1 text-xs text-wool-200 resize-none"></textarea>
                </template>
                <template x-if="editingCell?.rowId !== row.id || editingCell?.column !== 'spec'">
                  <div @dblclick="startEditing(island.id, row.id, 'spec')"
                       class="text-xs text-wool-300 cursor-pointer hover:bg-pasture-700/50 p-1 rounded"
                       x-html="renderMarkdown(row.spec_content || 'Click to add spec...')"></div>
                </template>
              </div>

              <!-- Task column -->
              <div class="p-2 border-r border-pasture-600 min-h-[60px]">
                <template x-if="row.task_title">
                  <div class="flex items-start gap-2">
                    <span class="w-2 h-2 mt-1 rounded-full flex-shrink-0"
                          :class="getTaskStatusDotClass(row.task_status)"></span>
                    <div class="flex-1">
                      <div class="text-xs font-medium text-wool-200"
                           :class="row.task_status === 'deleted' ? 'line-through opacity-50' : ''"
                           x-text="row.task_title"></div>
                      <div class="text-xs text-wool-400" x-text="row.task_worker || ''"></div>
                      <!-- Show run link if dispatched -->
                      <template x-if="row.dispatched && row.run_name">
                        <a @click.stop="selectRun(row.run_name)"
                           class="text-xs text-amber-400 hover:underline cursor-pointer">
                          → <span x-text="row.run_name"></span>
                        </a>
                      </template>
                    </div>
                  </div>
                </template>
                <template x-if="!row.task_title">
                  <button @click="startEditing(island.id, row.id, 'task')"
                          class="text-xs text-wool-500 hover:text-wool-300">
                    + Add task
                  </button>
                </template>
              </div>

              <!-- Eval column -->
              <div class="p-2 min-h-[60px]">
                <div class="flex items-center gap-2">
                  <span class="text-sm"
                        x-text="row.eval_status === 'pass' ? '🟢' : row.eval_status === 'fail' ? '🔴' : '⚪'"></span>
                  <div @dblclick="startEditing(island.id, row.id, 'eval')"
                       class="text-xs text-wool-300 cursor-pointer flex-1"
                       x-text="row.eval_criterion || 'Click to add criterion...'"></div>
                </div>
              </div>

            </div>
          </template>

          <!-- Review card (shown when run is complete) -->
          <template x-if="island.rows.some(r => r.dispatched && r.run_name)">
            <div class="p-2 bg-pasture-700/30 border-t border-pasture-600">
              <template x-for="runName in [...new Set(island.rows.filter(r => r.dispatched).map(r => r.run_name))]" :key="runName">
                <div class="flex items-center justify-between text-xs py-1">
                  <span class="text-wool-300">
                    Run: <span class="text-amber-400" x-text="runName"></span>
                  </span>
                  <div class="flex gap-2">
                    <button @click="selectRun(runName)" class="text-wool-400 hover:text-wool-200">View</button>
                    <button @click="deliverRun(runName)" class="text-sage-400 hover:text-sage-300">Deliver</button>
                  </div>
                </div>
              </template>
            </div>
          </template>

          <!-- Add row button -->
          <button @click="addRow(island.id)"
                  class="w-full py-2 text-xs text-wool-500 hover:text-wool-300 hover:bg-pasture-700/50">
            + Add row
          </button>
        </div>

      </div>
    </template>

  </div>

  <!-- HUD: Toolbar -->
  <div class="absolute top-4 left-4 z-20 flex gap-2">
    <button @click="createIsland($refs.boardContainer.clientWidth / 2, $refs.boardContainer.clientHeight / 2)"
            class="btn-sm bg-pasture-700 hover:bg-pasture-600 text-wool-200">
      <i data-lucide="plus" class="w-4 h-4"></i>
    </button>
    <button @click="fitAll()" class="btn-sm bg-pasture-700 hover:bg-pasture-600 text-wool-200">
      <i data-lucide="maximize" class="w-4 h-4"></i>
    </button>
    <button @click="toggleRunMode()"
            :class="runMode ? 'bg-amber-500 text-white' : 'bg-pasture-700 text-wool-200'"
            class="btn-sm hover:opacity-90">
      <i data-lucide="play" class="w-4 h-4"></i>
      <span x-show="runMode" class="ml-1" x-text="`(${selectedRowsForDispatch.size})`"></span>
    </button>
    <button x-show="runMode && selectedRowsForDispatch.size > 0"
            @click="dispatchSelectedRows()"
            class="btn-sm bg-sage-600 hover:bg-sage-500 text-white">
      Create Draft
    </button>
  </div>

  <!-- Dispatch Warning Modal -->
  <div x-show="dispatchWarning" x-transition
       @click.self="cancelDispatchWarning()"
       class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center">
    <div class="bg-pasture-800 rounded-lg shadow-xl border border-pasture-600 w-96 p-4">
      <div class="flex items-start gap-3 mb-4">
        <i data-lucide="alert-triangle" class="w-5 h-5 text-golden-400 flex-shrink-0 mt-0.5"></i>
        <div>
          <h3 class="font-medium text-wool-200">Already Dispatched</h3>
          <p class="text-sm text-wool-400 mt-1" x-text="dispatchWarning?.message"></p>
        </div>
      </div>
      <div class="flex justify-end gap-2">
        <button @click="cancelDispatchWarning()" class="btn-sm btn-ghost">Cancel</button>
        <button @click="confirmDispatchWithWarning()" class="btn-sm bg-amber-500 text-white">
          Dispatch Anyway
        </button>
      </div>
    </div>
  </div>

  <!-- HUD: Zoom indicator -->
  <div class="absolute top-4 right-4 z-20 text-xs text-wool-400 bg-pasture-800/80 px-2 py-1 rounded">
    <span x-text="Math.round(transform.k * 100)"></span>%
  </div>

  <!-- Cmd+K Palette -->
  <div x-show="cmdKOpen" x-transition
       @click.self="cmdKOpen = false"
       class="fixed inset-0 z-50 bg-black/50 flex items-start justify-center pt-32">
    <div class="bg-pasture-800 rounded-lg shadow-xl border border-pasture-600 w-96">
      <input type="text"
             x-ref="cmdKInput"
             @keydown.escape="cmdKOpen = false"
             placeholder="Jump to feature..."
             class="w-full px-4 py-3 bg-transparent border-b border-pasture-600 text-wool-200 outline-none">
      <div class="max-h-64 overflow-y-auto">
        <template x-for="island in islands" :key="island.id">
          <button @click="jumpToIsland(island.id)"
                  class="w-full px-4 py-2 text-left text-sm text-wool-200 hover:bg-pasture-700 flex items-center gap-2">
            <span class="w-2 h-2 rounded-full" :class="getStatusDotClass(island.status)"></span>
            <span x-text="island.name"></span>
          </button>
        </template>
      </div>
    </div>
  </div>

</div>
```

---

## 4. Dependencies

### 4.1 New npm packages

```json
{
  "dependencies": {
    "d3-zoom": "^3.0.0",
    "d3-selection": "^3.0.0"
  },
  "devDependencies": {
    "@types/d3-zoom": "^3.0.0",
    "@types/d3-selection": "^3.0.0"
  }
}
```

### 4.2 Rust crates (no new dependencies needed)

The existing `rusqlite`, `serde`, `uuid` crates are sufficient.

---

## 5. Implementation Phases

### Phase 1: Data Model & Basic Board ✅
- [x] Create `specflow/` module with SQLite schema
- [x] Add Tauri commands for CRUD operations
- [x] Create basic `specflowBoard()` Alpine component
- [x] Implement d3-zoom pan/zoom
- [x] Canvas grid background
- [x] Static island rendering (no LOAD yet)
- [x] Update sidebar to show projects (Board section)
- [x] View switching logic in `index.html`

### Phase 2: Trifecta Grid & Editing ✅
- [x] Implement 3-column Trifecta Grid layout
- [x] Row CRUD operations
- [x] Hollow Block editing (textarea on focus)
- [x] Markdown rendering in cells (basic)
- [x] Island drag-to-reposition
- [x] Auto-save on blur

### Phase 3: LOAD & Virtualization ✅
- [x] Three LOAD views (far/mid/near)
- [x] Viewport culling (only render visible islands)
- [ ] AI summary generation for LOAD mode (deferred)
- [x] Performance optimization

### Phase 4: Wires & Dependencies ✅
- [x] Canvas wire rendering (bezier curves with arrowheads)
- [x] Wire CRUD operations
- [x] Dependency data model (task_blocked_by)
- [x] Ripple selection in run mode

### Phase 5: Run Dispatch Integration ✅
- [x] Run mode toggle
- [x] Row-level multi-select (not island-level)
- [x] Dispatch to run (generate spec/eval/tasks)
- [x] Sync run status back to rows
- [x] Review panel for dispatched rows
- [x] Dispatch warning modal for already-dispatched rows

### Phase 6: Navigation & Polish ✅
- [x] Cmd+K jump-to palette
- [x] Bookmarks (API ready, UI in template)
- [x] Fit-all button
- [x] Keyboard shortcuts (R for run mode, Escape, Cmd+K)
- [ ] Gyp AI context integration (deferred)

---

## 6. Architecture Quick Reference Update

Add to `docs/architecture.md`:

| Task | Files to Modify |
|------|-----------------|
| Add SpecFlow island | `src-tauri/src/core/specflow/state.rs` |
| Add SpecFlow command | `src-tauri/src/gui/commands/specflow.rs` |
| Modify board UI | `src/lib/components/specflow-board/index.ts`, `src/templates/specflow-board.html` |
| Change canvas rendering | `src/lib/components/specflow-board/canvas-renderer.ts` |

---

## 7. Open Questions

1. **Project creation flow**: Where/how should users create new projects? Modal from sidebar? Separate "New Project" screen?

2. **Migration from existing projects**: Should we auto-create islands from existing project specs, or start fresh?

3. **Gyp AI integration depth**: Should Gyp be able to:
   - Create/modify islands based on natural language?
   - Auto-generate tasks from specs?
   - Fill in eval criteria?

4. **Asset management**: The original spec mentions image pasting. Should we support images in spec cells? (Can use existing asset manager pattern)
