//! Centralized constants for hirsel.
//!
//! This module contains application-wide constants that are used across
//! multiple modules. Module-specific constants (like schema versions or
//! name lists) remain in their respective modules.

use std::time::Duration;

// =============================================================================
// Model Configuration
// =============================================================================

/// Context window sizes per model (in tokens)
pub const CONTEXT_WINDOWS: &[(&str, u32)] = &[("gpt-5", 200_000), ("gpt-5-mini", 200_000)];

/// Default context window size for unknown models
pub const DEFAULT_CONTEXT_WINDOW: u32 = 200_000;

// =============================================================================
// Agent Timeouts
// =============================================================================

/// Default timeout for agent operations (5 minutes)
pub const DEFAULT_AGENT_TIMEOUT_SECS: u64 = 300;

/// Timeout for conflict resolution agent (5 minutes)
pub const RESOLUTION_TIMEOUT_SECS: u64 = 300;

// =============================================================================
// Eval System
// =============================================================================

/// Maximum number of times to prompt the agent if it doesn't submit a verdict
pub const MAX_VERDICT_RETRIES: usize = 2;

/// Reminder prompt sent if agent doesn't submit verdict
pub const VERDICT_REMINDER_PROMPT: &str = r#"You have not yet submitted your evaluation verdict.

You MUST call one of these MCP tools to complete your evaluation:

- `mcp__eval__eval_pass` - if all checks passed
- `mcp__eval__eval_fail` - if any check failed (include feedback parameter)

Please call the appropriate tool NOW to submit your verdict."#;

// =============================================================================
// Metrics
// =============================================================================

/// Cache time-to-live for session metrics
pub const METRICS_CACHE_TTL: Duration = Duration::from_secs(5);

// =============================================================================
// Worker Notifications
// =============================================================================

/// Time notification thresholds as percentage of elapsed time (accelerating frequency)
pub const TIME_NOTIFICATION_THRESHOLDS: &[i64] = &[25, 50, 75, 85, 90, 95, 98];

// =============================================================================
// Agent Session Paths
// =============================================================================

/// Agent session directory name.
pub const AGENT_SESSION_DIR: &str = ".codex";

// =============================================================================
// Conflict Resolution
// =============================================================================

/// Prompt template for the conflict resolver agent
pub const RESOLVER_PROMPT: &str = r#"You are resolving git merge conflicts.

## Context

The following files have merge conflicts with conflict markers (<<<<<<, =======, >>>>>>>):

{files}

## Task Context

{context}

## Instructions

1. Read each conflicting file
2. Understand both versions of the changes
3. Make a semantic choice about how to combine or resolve the conflicts
4. Remove ALL conflict markers (<<<<<<, =======, >>>>>>>) from each file
5. Write the resolved version back to each file
6. Use `git add <file>` for each resolved file

## Important Rules

- NEVER leave conflict markers in files
- If unsure, prefer the incoming changes (after =======) as they are newer
- Keep all meaningful changes from both sides when possible
- After resolving all files, verify with `git status` that there are no unmerged files

## Conflicting Files

"#;

// =============================================================================
// Plan Task System
// =============================================================================

/// Prompt for plan worker tasks created during dispatch
pub const PLAN_TASK_PROMPT: &str = r#"You are a **planning worker** responsible for decomposing a spec into implementation tasks.

## Your Job

1. Read the target feature spec (shown above) with `get_task_details()` to understand what needs to be built
2. Read retained project context with `read_retained_context()` to understand stable constraints
3. Explore the codebase using filesystem tools to assess what exists vs what's needed
4. Create implementation tasks as children of the target feature (via `add_task()`)
5. Create checks to validate the implementation (via `add_check()`) — parent them under the feature they validate so they appear in the tree. Only omit parent for truly global/e2e checks.
6. Set `blocked_by` relationships between tasks where needed
7. Use `scribe()` for durable findings that should survive route churn
8. Call `work_done()` when planning is complete

## Task Design Principles

**Parallel execution:**
- Minimize dependencies between tasks
- Prefer vertical slices (complete features) over horizontal layers
- Tasks touching same files = conflicts. Structure to minimize overlap.

**Dependencies (blocked_by):**
When in doubt, add the dependency. Better slow than broken:
- Task reads files another writes? → Add dependency
- Task calls functions another creates? → Add dependency
- Task tests code another implements? → Add dependency

**Task granularity:**
- Each task should be completable by a single worker in one session
- Include enough context in the task description for an independent worker
- Reference specific files, functions, and patterns to modify

## Completing This Task

When you've created all implementation tasks and evals with proper dependencies, call `work_done()`.
Your completion unblocks the implementation tasks you created."#;

/// System prompt injected for plan workers (assigned plan tasks).
///
/// This complements PLAN_TASK_PROMPT (which workers see via get_task_details)
/// by providing system-level guidance on decomposition strategy.
pub const PLAN_WORKER_SYSTEM_PROMPT: &str = r#"# Plan Worker

You are a **planning worker**, not an implementation worker. Your job is to decompose a feature spec into concrete implementation tasks and validation checks that other workers will execute.

## Workflow

1. **Read the plan task details** — `get_task_details("<your_task_id>")` to see the target feature you're planning
2. **Read retained project context** — `read_retained_context()` for stable constraints and decisions
3. **Explore the codebase** — understand existing patterns, files, and conventions
4. **Create tasks** — `add_task()` for each implementation unit
5. **Create checks** — `add_check()` for validation/testing — parent under the feature so they appear in the tree. Only omit parent for global/e2e checks.
6. **Set dependencies** — use `blocked_by` on `add_task()` to order work correctly
7. **Update retained context** — call `scribe()` if you discover durable project constraints
8. **Finish** — call `work_done()` when all tasks and checks are created

## Design Principles

- **Maximize parallelism** — structure tasks so independent pieces can run concurrently
- **When in doubt, add a dependency** — better slow than broken
- **Granular tasks** — each task should be completable by one worker in one session
- **Full context in descriptions** — include specific files, functions, line numbers, and patterns
- **Vertical slices** — prefer complete features over horizontal layers
- **Minimize file overlap** — tasks touching the same files create merge conflicts

Do NOT write implementation code. Create tasks that describe what to implement."#;
