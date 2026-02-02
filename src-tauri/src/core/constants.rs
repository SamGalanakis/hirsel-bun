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
pub const CONTEXT_WINDOWS: &[(&str, u32)] = &[
    ("claude-opus-4-5-20251101", 200_000),
    ("claude-sonnet-4-5-20251101", 200_000),
    ("claude-sonnet-4-20250514", 200_000),
    ("claude-3-5-sonnet-20241022", 200_000),
    ("claude-3-5-haiku-20241022", 200_000),
    ("claude-3-opus-20240229", 200_000),
    ("claude-3-sonnet-20240229", 200_000),
    ("claude-3-haiku-20240307", 200_000),
];

/// Default context window size for unknown models
pub const DEFAULT_CONTEXT_WINDOW: u32 = 200_000;

// =============================================================================
// Agent Timeouts
// =============================================================================

/// Default timeout for ACP agent operations (5 minutes)
pub const DEFAULT_ACP_TIMEOUT_SECS: u64 = 300;

/// Timeout for scribe agent operations (2 minutes)
pub const SCRIBE_TIMEOUT_SECS: u64 = 120;

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
// External APIs
// =============================================================================

/// Fly.io Machines API base URL
pub const FLY_API_BASE: &str = "https://api.machines.dev/v1";

// =============================================================================
// Agent Session Paths
// =============================================================================

/// Claude's session directory name
pub const CLAUDE_SESSION_DIR: &str = ".claude";

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
// Scribe System
// =============================================================================

/// Prompt template for the Scribe agent
pub const SCRIBE_PROMPT: &str = r#"You are a documentation scribe maintaining developer reference docs.

## First: Adopt Existing Structure

Read docs/ first. If the project has its own documentation structure, adopt it.
Maintain consistency with what exists.

## Documentation Style

Write **developer reference** docs - help someone understand the system and find what they need.

**Good content:**
- High-level feature descriptions (what the system does)
- Technology stack and why each piece is used
- Quick reference tables (Task -> Files to Modify)
- Module/component maps with purposes
- Key abstractions (traits, interfaces, patterns)
- State machines and status flows
- Architecture diagrams (ASCII)
- Configuration options
- Data flow descriptions
- Gotchas, pitfalls, non-obvious constraints
- Style conventions (especially frontend: components, patterns, naming)

**Avoid:**
- Prose explanations (use tables and bullets)
- Implementation details that change often
- Code snippets or examples
- Tutorials or how-to guides
- Anything obvious from reading code

## Principles

- Structure over prose
- Help devs find the right place to look
- Document the shape of the system, not the details
- New info wins over old (update, don't duplicate)

Learnings to process:
"#;
