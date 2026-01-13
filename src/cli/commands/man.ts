/**
 * hirsel man - Show manual
 *
 * Displays the full manual with detailed command documentation.
 */

import { dim, bold, colorize, LOGO } from "../../shared/theme";

const MANUAL = `
${LOGO}
${bold("HIRSEL - Herd your AI coding agents")}

${bold("SYNOPSIS")}
    hirsel [command] [options]
    hirsel-worker <command> [options]

${bold("DESCRIPTION")}
    Hirsel is a tool for coordinating multiple AI coding agents working
    on software development tasks. It manages runs, tasks, workers, and
    communication between agents and humans.

    When invoked without arguments, hirsel launches the native desktop
    application for monitoring and managing runs.

${bold("COMMANDS")}

  ${bold("Run Management")}

    ${bold("go")} <run> <spec>
        Start a new run with the given spec file.

        Options:
          --workers N       Fixed N workers
          --workers 1-5     Autoscale between 1 and 5
          --workers 2+      Autoscale from 2 upward
          --time-limit      Time limit (30m, 1h, 1h30m)
          --project <path>  Project path (default: current directory)
          --template <name> Use a predefined template
          --sandbox         Run in sandbox mode
          --yolo            Disable human-in-the-loop confirmations

    ${bold("view")} <run>
        View detailed status of a run including workers, tasks,
        and recent activity.

    ${bold("log")} <run>
        View the activity log for a run.

        Options:
          -f, --follow     Follow log updates in real-time

    ${bold("attach")} <run> [worker]
        Attach to a worker's terminal session. If no worker is
        specified, shows a selection menu.

    ${bold("msg")} <run> <message>
        Send a message to the run's user thread. Workers can see
        and respond to these messages.

    ${bold("diff")} <run>
        Show git diff of changes made by workers.

    ${bold("deliver")} <run>
        Create a branch in the target repository with the
        completed work. Branch name: hirsel/<run-name>

    ${bold("pause")} <run>
        Pause all workers in a run.

    ${bold("resume")} <run>
        Resume a paused or timed-out run.

        Options:
          --time-limit     Set new time limit

    ${bold("delete")} <run>
        Remove a run and all its data.

    ${bold("prune")}
        Remove all delivered runs to free disk space.

    ${bold("runs")}
        List all runs with status, progress, and worker count.

        Options:
          --json           Output in JSON format

    ${bold("summary")} <run>
        Generate a summary of the run including completed tasks,
        worker activity, and AI-generated summary if available.

  ${bold("Task Management")}

    ${bold("tasks")} <run>
        List all tasks in a run.

    ${bold("task-add")} <run> <id> <description>
        Add a new task to a run.
        Task ID must be lowercase, start with letter, use underscores.

    ${bold("task-delete")} <run> <id>
        Delete a task from a run.

    ${bold("task-done")} <run> <id>
        Mark a task as completed.

    ${bold("task-reopen")} <run> <id>
        Reopen a completed task.

    ${bold("task-unclaim")} <run> <id>
        Release a claimed task.

  ${bold("Configuration")}

    ${bold("config")} [agent]
        Configure which AI agent to use. Without arguments, shows
        an interactive selection menu.

        Available agents:
          claude    Anthropic Claude Code (default)
          gemini    Google Gemini CLI
          opencode  OpenCode
          codex     OpenAI Codex CLI
          goose     Block Goose

    ${bold("templates")}
        List available spec templates.

    ${bold("completions")} [bash|zsh]
        Output shell completion scripts.

    ${bold("man")}
        Show this manual.

${bold("WORKER COMMANDS")}

    These commands are used by AI agents via MCP. They operate on
    the current run (set via HIRSEL_RUN environment variable).

    ${bold("hirsel-worker task-claim")} <id>
        Claim a task to work on.

    ${bold("hirsel-worker task-done")} [id]
        Mark the current task as completed.

    ${bold("hirsel-worker task-add")} <id> <description>
        Add a follow-up task.

    ${bold("hirsel-worker task-await")}
        Wait for available tasks (blocking).

    ${bold("hirsel-worker msg")} <thread> <message>
        Send a message to a thread.

${bold("ENVIRONMENT")}

    HIRSEL_ROOT
        Base directory for hirsel data (default: ~/.hirsel)

    HIRSEL_RUN
        Current run name (set automatically for workers)

    HIRSEL_WORKER
        Worker name (set automatically for workers)

${bold("FILES")}

    ~/.hirsel/
        Base directory for all hirsel data

    ~/.hirsel/config.toml
        Configuration file

    ~/.hirsel/runs/<name>/
        Run-specific data directory

    ~/.hirsel/runs/<name>/hirsel.db
        SQLite database with run state

${bold("EXAMPLES")}

    Start a new run:
        hirsel go feature-x ./spec.md --workers 3

    Watch progress:
        hirsel view feature-x

    Attach to a worker:
        hirsel attach feature-x

    Deliver completed work:
        hirsel deliver feature-x

${bold("SEE ALSO")}

    https://github.com/yourusername/hirsel

`;

// Main command handler
export default async function man(args: string[]): Promise<void> {
  console.log(MANUAL);
}
