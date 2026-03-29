use std::path::{Path, PathBuf};

/// Configuration for running a worker.
#[derive(Debug, Clone)]
pub struct WorkerRunConfig {
    pub runtime_name: String,
    pub worker_name: String,
    pub work_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub agent_command: Vec<String>,
    pub is_leader: bool,
    pub leader_name: Option<String>,
    pub teammates: Option<Vec<String>>,
    pub resume_session_id: Option<String>,
    /// Task ID assigned to this worker (direct task assignment)
    pub assigned_task_id: Option<String>,
    /// Whether the assigned task is a plan task (NodeKind::Plan)
    pub is_plan_task: bool,
}

/// Build the worker prompt with all context.
///
/// Workers access task details via tools (get_task_tree, get_task_details, etc.)
pub fn build_worker_prompt(
    worker_name: &str,
    runtime_name: &str,
    teammates: Option<&[String]>,
    work_dir: &Path,
    runtime_dir: &Path,
    assigned_task_id: Option<&str>,
    is_plan_task: bool,
) -> String {
    let is_multi_worker = teammates.map(|t| !t.is_empty()).unwrap_or(false);

    let mut prompt = String::new();

    // Plan worker system prompt (injected before generic worker instructions)
    if is_plan_task {
        prompt.push_str(crate::backend::constants::PLAN_WORKER_SYSTEM_PROMPT);
        prompt.push_str("\n\n---\n\n");
    }

    // Header
    prompt.push_str("# Hirsel Worker Mode\n\n");
    prompt.push_str("You are an autonomous worker executing a defined task.\n\n");

    // Context
    prompt.push_str("## Your Context\n\n");
    prompt.push_str(&format!("- **Worker name:** {}\n", worker_name));
    prompt.push_str(&format!("- **Run name:** {}\n", runtime_name));
    if let Some(task_id) = assigned_task_id {
        prompt.push_str(&format!("- **Assigned task:** `{}`\n", task_id));
    }
    prompt.push_str(&format!(
        "- **Work directory:** {} (git worktree - write code here)\n",
        work_dir.display()
    ));
    prompt.push_str(&format!("- **Run directory:** {}\n", runtime_dir.display()));
    prompt.push_str(&format!(
        "- **Assets directory:** {} (images & files referenced in spec)\n\n",
        runtime_dir.join("assets").display()
    ));

    // Your Task - Direct assignment
    prompt.push_str("## Your Assigned Task\n\n");
    if let Some(task_id) = assigned_task_id {
        prompt.push_str(&format!("**Task:** `{}`\n\n", task_id));
        prompt.push_str("Use `get_task_details(\"");
        prompt.push_str(task_id);
        prompt.push_str("\")` to see your task content and requirements.\n\n");
    } else {
        prompt.push_str("Check `get_my_tasks()` to see your assigned work.\n\n");
    }
    prompt.push_str("**Task tools:**\n");
    prompt.push_str("- `get_task_details(task_id)` - Get full content for a task\n");
    prompt.push_str("- `get_task_tree()` - See all tasks and their relationships\n");
    prompt.push_str("- `get_available_tasks()` - See tasks ready to work on\n\n");

    // Git workflow
    prompt.push_str("## Git Workflow\n\n");

    if is_multi_worker {
        // Multi-worker: isolated clone with origin pointing to shared staging
        prompt.push_str("You're working on a `staging` branch in your own isolated workspace.\n");
        prompt.push_str("Your `origin` remote points to the shared staging repository.\n\n");
    } else {
        // Single-worker: working directly in staging workspace, no remote
        prompt.push_str("You're working directly on the `staging` branch.\n");
        prompt.push_str("This is a single-worker route workspace - no git remote is configured.\n");
        prompt.push_str(
            "Your changes stay local until the orchestrator chooses how to continue.\n\n",
        );
    }

    prompt.push_str("**Commit Discipline (IMPORTANT):**\n");
    prompt.push_str("- **One commit per logical change** - atomic commits make debugging easy\n");
    prompt.push_str("- **Commit immediately after each change works** - don't batch changes\n");
    prompt.push_str("- **Descriptive messages** - use conventional format: `feat:`, `fix:`, `test:`, `refactor:`\n\n");
    prompt.push_str("```bash\n");
    prompt.push_str("# Good: commit as you go\n");
    prompt.push_str("git add src/auth.py && git commit -m \"feat: add password hashing\"\n");
    prompt.push_str("git add tests/ && git commit -m \"test: add auth unit tests\"\n\n");
    prompt.push_str("# Bad: one commit with everything\n");
    prompt.push_str("git add . && git commit -m \"Add authentication\"  # DON'T DO THIS\n");
    prompt.push_str("```\n\n");

    if is_multi_worker {
        // Multi-worker: must push to share changes
        prompt.push_str("**Before Completing a Task:**\n");
        prompt.push_str(
            "You MUST push your changes to the shared staging before calling `complete_task`:\n",
        );
        prompt.push_str("```bash\n");
        prompt
            .push_str("git add . && git commit -m \"feat: final changes\"  # if any uncommitted\n");
        prompt
            .push_str("git pull origin staging                            # get others' changes\n");
        prompt.push_str("# resolve any conflicts if needed, then:\n");
        prompt.push_str("git push origin staging                            # share your work\n");
        prompt.push_str("```\n");
        prompt.push_str("Only call `complete_task` AFTER your changes are pushed.\n\n");

        prompt.push_str("**Handling Merge Conflicts:**\n");
        prompt.push_str("If `git pull` shows conflicts:\n");
        prompt.push_str(
            "1. Edit conflicted files (remove `<<<<<<<`, `=======`, `>>>>>>>` markers)\n",
        );
        prompt.push_str("2. `git add <resolved-files>`\n");
        prompt.push_str("3. `git commit`\n");
        prompt.push_str("4. `git push origin staging`\n\n");
    } else {
        // Single-worker: just commit, no push needed
        prompt.push_str("**Before Completing a Task:**\n");
        prompt.push_str("Commit any uncommitted changes before calling `complete_task`:\n");
        prompt.push_str("```bash\n");
        prompt.push_str("git add . && git commit -m \"feat: final changes\"\n");
        prompt.push_str("```\n");
        prompt.push_str("No git push is needed - your changes are already in the workspace.\n\n");
    }

    prompt.push_str("## Available Tools\n\n");
    prompt.push_str("Use these tools directly; do not rely on local hirsel CLI binaries.\n\n");

    prompt.push_str("### Task Management\n");
    prompt.push_str("- `get_task_tree()` - Full task hierarchy with status and dependencies\n");
    prompt.push_str("- `get_available_tasks()` - Unblocked, unclaimed tasks ready to work on\n");
    prompt.push_str("- `get_my_tasks()` - Tasks you've claimed\n");
    prompt.push_str("- `get_task_details(task_id)` - Full content for a specific task\n");
    prompt.push_str("- `complete_task(task_id?)` - Mark task done (auto-unblocks dependents)\n");
    prompt.push_str("- `add_task(task_id, name, parent?, blocked_by?)` - Create a new task\n");
    prompt.push_str("- `delete_task(task_id)` - Delete a worker-created task\n");
    prompt.push_str("- `add_check(check_id, name, validates?)` - Create check task\n\n");

    prompt.push_str("### Orchestrator Coordination\n");
    prompt.push_str(
        "- `report_progress(summary, details?)` - Send a non-blocking progress update upward\n",
    );
    prompt.push_str("- `raise_concern(kind, summary, details?, severity?, blocking?)` - Escalate a blocker, risk, conflict, or review need to the orchestrator\n");
    prompt.push_str("- `request_decision(summary, details?)` - Pause and ask the orchestrator for a decision you cannot safely make alone\n\n");

    prompt.push_str("### Retained Context\n");
    prompt.push_str("- `read_retained_context()` - Read durable project context\n");
    prompt.push_str("- `scribe(content)` - Record durable context for scribe condensation\n\n");

    prompt.push_str("### Completion\n");
    prompt
        .push_str("- `work_done` - Signal task complete and return control to the orchestrator\n");
    prompt.push_str("- `time_status` - Check time limit status\n\n");

    prompt.push_str("### Check Operations\n");
    prompt.push_str("- `check_pass()` - Mark check as passed (only for check tasks)\n");
    prompt.push_str("- `check_fail(feedback)` - Mark check as failed with feedback\n\n");

    // Task statuses
    prompt.push_str("## Task Workflow\n\n");
    prompt.push_str("**Statuses:**\n");
    prompt.push_str("- `TODO` - Available to work on\n");
    prompt.push_str("- `DOING` - Assigned to a worker\n");
    prompt.push_str("- `DONE` - Completed\n");
    prompt.push_str("- `BLOCKED` - Waiting for dependencies\n\n");
    prompt.push_str("**Direct Task Assignment:**\n");
    prompt.push_str("- Your task is **pre-assigned** when you spawn - no need to claim\n");
    prompt.push_str("- Use `get_task_details(task_id)` to see full task content\n");
    prompt.push_str("- Complete the work in your git workspace\n");
    prompt.push_str("- Call `work_done()` when your task is complete\n");
    prompt.push_str(
        "  - This marks your task done, exits, and returns control to the orchestrator\n\n",
    );
    prompt.push_str("**Creating subtasks:**\n");
    prompt.push_str("- You can still use `add_task()` to break down work\n");
    prompt.push_str("- Subtasks go into the pool and may be assigned to you or other workers\n\n");

    // Escalation section
    prompt.push_str("## Escalation\n\n");
    prompt.push_str("You do not message the user or other workers directly. The orchestrator owns all user communication and cross-worker coordination.\n\n");
    prompt.push_str("**Use `report_progress()` when:**\n");
    prompt.push_str("- You finished a meaningful sub-result worth surfacing upward\n");
    prompt.push_str(
        "- You discovered something the orchestrator should know while you continue working\n\n",
    );
    prompt.push_str("**Use `raise_concern()` when:**\n");
    prompt.push_str("- You found a blocker, risk, conflict, or review need\n");
    prompt.push_str("- You suspect the route/task plan should change\n");
    prompt.push_str("- Another worker's work or route state may invalidate your current plan\n\n");
    prompt.push_str("**Use `request_decision()` when:**\n");
    prompt.push_str("- You truly need a decision before you can continue safely\n");
    prompt.push_str("- The spec is ambiguous in a way that would materially affect the outcome\n");
    prompt.push_str("- A tradeoff needs orchestrator or user judgment\n\n");
    prompt.push_str("```text\n");
    prompt.push_str("request_decision(\"Choose database\", \"I can implement either PostgreSQL or SQLite; current constraints are unclear.\")\n");
    prompt.push_str("```\n\n");

    prompt.push_str("## Retained Context\n\n");
    prompt.push_str("Call `read_retained_context()` before major work to understand durable project constraints.\n");
    prompt.push_str("Call `scribe()` only for context that should survive route churn, not for transient progress notes.\n\n");

    // Getting started
    prompt.push_str("## Getting Started\n\n");
    prompt.push_str("1. Use `get_task_details(your_assigned_task)` to see your task\n");
    prompt.push_str("2. Work on the task in your git workspace\n");
    prompt.push_str("3. Commit your changes\n");
    prompt.push_str("4. Call `work_done()` - your task is auto-completed and the orchestrator decides the next delegation\n\n");

    // When stuck
    prompt.push_str("## When Stuck\n\n");
    prompt.push_str("Don't spin. If you can't figure something out after 2-3 attempts, raise a structured concern or request a decision instead of guessing blindly.\n\n");

    prompt.push_str("**Begin by reviewing your assigned task with `get_task_details()`.**\n");

    prompt
}
