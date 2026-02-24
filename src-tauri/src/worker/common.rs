use std::path::{Path, PathBuf};

/// Configuration for running a worker.
#[derive(Debug, Clone)]
pub struct WorkerRunConfig {
    pub run_name: String,
    pub worker_name: String,
    pub work_dir: PathBuf,
    pub run_dir: PathBuf,
    pub agent_command: Vec<String>,
    pub is_leader: bool,
    pub leader_name: Option<String>,
    pub teammates: Option<Vec<String>>,
    pub resume_session_id: Option<String>,
    /// Optional API URL for reporting status (used by Docker/remote workers)
    pub api_url: Option<String>,
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
    run_name: &str,
    teammates: Option<&[String]>,
    work_dir: &Path,
    run_dir: &Path,
    assigned_task_id: Option<&str>,
    is_plan_task: bool,
) -> String {
    let is_multi_worker = teammates.map(|t| !t.is_empty()).unwrap_or(false);

    let mut prompt = String::new();

    // Plan worker system prompt (injected before generic worker instructions)
    if is_plan_task {
        prompt.push_str(crate::core::constants::PLAN_WORKER_SYSTEM_PROMPT);
        prompt.push_str("\n\n---\n\n");
    }

    // Header
    prompt.push_str("# Hirsel Worker Mode\n\n");
    prompt.push_str("You are an autonomous worker executing a defined task.\n\n");

    // Context
    prompt.push_str("## Your Context\n\n");
    prompt.push_str(&format!("- **Worker name:** {}\n", worker_name));
    prompt.push_str(&format!("- **Run name:** {}\n", run_name));
    if let Some(task_id) = assigned_task_id {
        prompt.push_str(&format!("- **Assigned task:** `{}`\n", task_id));
    }
    prompt.push_str(&format!(
        "- **Work directory:** {} (git worktree - write code here)\n",
        work_dir.display()
    ));
    prompt.push_str(&format!("- **Run directory:** {}\n", run_dir.display()));
    prompt.push_str(&format!(
        "- **Assets directory:** {} (images & files referenced in spec)\n\n",
        run_dir.join("assets").display()
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
        prompt.push_str("This is a single-worker run - no git remote is configured.\n");
        prompt.push_str("Your changes stay local until the run completes.\n\n");
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

    prompt.push_str("### Communication\n");
    prompt
        .push_str("- `list_contacts()` - Available chat targets (user, group, workers, scribe)\n");
    prompt.push_str("- `chat_history(with?, limit?)` - Read message history\n");
    prompt.push_str("- `chat_send(to, message)` - Send a message\n");
    prompt.push_str("- `chat_unread(with?)` - Check for new unread messages\n\n");

    prompt.push_str("### Documentation\n");
    prompt.push_str("- `scribe(content)` - Record a learning or discovery\n");
    prompt.push_str("- `read_docs(file?)` - Read project documentation\n\n");

    prompt.push_str("### Completion\n");
    prompt.push_str("- `work_done` - Signal task complete and ready for new assignment\n");
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
    prompt.push_str("  - This marks your task done and exits\n");
    prompt.push_str("  - You'll be respawned with a new task if one is available\n\n");
    prompt.push_str("**Creating subtasks:**\n");
    prompt.push_str("- You can still use `add_task()` to break down work\n");
    prompt.push_str("- Subtasks go into the pool and may be assigned to you or other workers\n\n");

    // Messaging section
    prompt.push_str("## Messaging the User\n\n");
    prompt.push_str("Messages to `user` pause execution until they reply.\n\n");
    prompt.push_str("**When to message:**\n");
    prompt.push_str("- You need information to proceed\n");
    prompt.push_str("- Significant architectural decision\n");
    prompt.push_str("- Spec is ambiguous\n");
    prompt.push_str("- Something the user should review\n\n");
    prompt.push_str("```\n");
    prompt
        .push_str("chat_send(\"user\", \"Which database should I use - PostgreSQL or SQLite?\")\n");
    prompt.push_str("```\n\n");

    // Documentation
    prompt.push_str("## Project Documentation\n\n");
    prompt
        .push_str("Use `scribe(content)` to record discoveries. A Scribe agent maintains docs/:\n");
    prompt.push_str("- `architecture.md` - System design, module relationships\n");
    prompt.push_str("- `patterns.md` - Code patterns and conventions\n");
    prompt.push_str("- `gotchas.md` - Pitfalls and things to watch out for\n");
    prompt.push_str("- `decisions.md` - Key decisions and rationale\n\n");
    prompt.push_str("**At task start:** Call `read_docs()` to check accumulated knowledge.\n");
    prompt.push_str("**During work:** Call `scribe()` when you discover something useful.\n\n");

    // Getting started
    prompt.push_str("## Getting Started\n\n");
    prompt.push_str("1. Use `get_task_details(your_assigned_task)` to see your task\n");
    prompt.push_str("2. Work on the task in your git workspace\n");
    prompt.push_str("3. Commit your changes\n");
    prompt.push_str("4. Call `work_done()` - your task is auto-completed and you'll be respawned with a new task if available\n\n");

    // When stuck
    prompt.push_str("## When Stuck\n\n");
    prompt.push_str("Don't spin. If you can't figure something out after 2-3 attempts:\n");
    prompt.push_str("```\n");
    prompt.push_str("chat_send(\"user\", \"Specific question about what's blocking you\")\n");
    prompt.push_str("```\n\n");

    prompt.push_str("**Begin by reviewing your assigned task with `get_task_details()`.**\n");

    prompt
}
