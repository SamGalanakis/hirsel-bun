//! Manual command implementation for hirsel.
//!
//! Shows the hirsel manual with ASCII sheep art and command documentation.

use crate::cli::ManArgs;

/// ASCII art sheep for the manual header
const SHEEP_ART: &str = r#"
          __  _
      .-.'  `; `-._  __  _
     (_,         .-:'  `; `-._
   ,'o"(        (_,           )
  (__,-'      ,'o"(            )>
     (       (__,-'            )
      `-'._.--._(             )
         |||  |||`-'._.--._.-'
                    |||  |||
"#;

/// The hirsel manual content
const MANUAL: &str = r#"
HIRSEL - Herd your AI coding agents

DESCRIPTION
    Hirsel orchestrates multiple AI coding agents working in parallel on
    software engineering tasks. Each agent works in its own git worktree,
    allowing true parallel development without merge conflicts.

    Think of it as a shepherd overseeing a flock of AI agents - each sheep
    (agent) works independently, but the shepherd (hirsel) coordinates their
    efforts toward a common goal.

QUICK START
    1. Create a spec file describing what you want built
    2. Run: hirsel go my-project spec.md --workers 3
    3. Watch agents work: hirsel attach my-project
    4. Deliver results: hirsel deliver my-project

RUN MANAGEMENT
    hirsel go <run> <spec>      Start a new run
        --workers N             Number of parallel workers (default: 1)
        --time-limit 30m        Optional time limit
        --yolo                  Skip confirmation prompts

    hirsel view <run>           View run status
    hirsel attach <run>         Watch worker live output (TUI)
    hirsel log <run> [-f]       View activity log
    hirsel diff <run>           Show code changes (git diff)
    hirsel pause <run>          Pause all workers
    hirsel resume <run>         Resume paused run
    hirsel deliver <run>        Push branch to original repo
    hirsel delete <run>         Remove a run
    hirsel prune                Remove all delivered runs
    hirsel runs                 List all runs
    hirsel summary <run>        Generate/view run summary

SPEC & MODE
    hirsel spec <run>           View run specification
    hirsel spec <run> --edit    Edit spec in $EDITOR
    hirsel amend <run> <text>   Add amendment to spec
    hirsel mode <run> hitl      Set human-in-the-loop mode
    hirsel mode <run> yolo      Set autonomous mode

MESSAGING
    hirsel msg <run> <message>  Send message to run
        --thread <name>         Thread name (default: user)
        --list-threads          List available threads

TASK MANAGEMENT (Admin)
    hirsel tasks <run>          List tasks
    hirsel task-add <run> <id> <desc>
                                Add a task
    hirsel task-delete <run> <id>
                                Delete a task
    hirsel task-done <run> <id> Mark task done
    hirsel task-reopen <run> <id>
                                Reopen completed task
    hirsel task-unclaim <run> <id>
                                Unclaim a task from worker

CONFIGURATION
    hirsel config [agent]       Configure agent (interactive or direct)
    hirsel templates            List spec templates
    hirsel completions          Install shell completions
    hirsel man                  Show this manual
    hirsel reset --runs         Delete all runs (requires typing 'reset')
    hirsel reset --config       Reset config to defaults
    hirsel reset --all          Reset everything

TESTING
    hirsel test <scenario>      Run e2e test scenario
        --list                  List available scenarios

WORKER COMMANDS (for AI agents)
    hirsel-worker task list     List all tasks
    hirsel-worker task claim <id>
                                Claim a task to work on
    hirsel-worker task done [id]
                                Mark current/specified task done
    hirsel-worker task undone <id>
                                Reopen a completed task
    hirsel-worker task add <id> <name>
                                Add follow-up task
    hirsel-worker task delete <id>
                                Delete a task
    hirsel-worker task unclaim [id]
                                Release current/specified task
    hirsel-worker task await    Wait for tasks to become available

    hirsel-worker msg send <thread> <msg>
                                Send message to thread
    hirsel-worker msg read [thread]
                                Read messages from thread
    hirsel-worker msg list      List available threads
    hirsel-worker msg inbox     Check for new messages

    hirsel-worker done          Signal all work complete

ENVIRONMENT
    HIRSEL_RUN                  Current run name (set for workers)
    HIRSEL_WORKER               Current worker name (set for workers)
    ANTHROPIC_API_KEY           API key for Claude agent
    GEMINI_API_KEY              API key for Gemini agent

FILES
    ~/.hirsel/                  Hirsel root directory
    ~/.hirsel/config.toml       Global configuration
    ~/.hirsel/runs/<run>/       Run-specific data
    ~/.hirsel/runs/<run>/spec.md
                                Run specification
    ~/.hirsel/runs/<run>/tasks.md
                                Task list
    ~/.hirsel/runs/<run>/chats/
                                Chat threads
    ~/.hirsel/runs/<run>/work/
                                Worker worktrees

AGENTS
    Supported AI coding agents:
    - claude    Claude Code (default)
    - gemini    Gemini Code
    - codex     OpenAI Codex
    - goose     Goose AI
    - opencode  OpenCode

    Configure with: hirsel config <agent>

EXAMPLES
    # Start a simple run
    hirsel go fix-bugs spec.md

    # Start with multiple workers and time limit
    hirsel go feature-impl spec.md --workers 3 --time-limit 1h

    # Watch a worker in action
    hirsel attach feature-impl

    # Send a message to the agents
    hirsel msg feature-impl "Please prioritize the login feature"

    # Check run status
    hirsel view feature-impl

    # Deliver completed work to your repo
    hirsel deliver feature-impl

VERSION
    hirsel {version}

AUTHORS
    Built with love for AI-assisted software development.

SEE ALSO
    https://github.com/anthropics/hirsel
"#;

/// Run the manual command
pub fn run_man(args: &ManArgs) -> anyhow::Result<()> {
    // Print sheep art
    println!("{}", SHEEP_ART);

    // Print manual
    let manual = MANUAL.replace("{version}", env!("CARGO_PKG_VERSION"));
    println!("{}", manual);

    if args.agent {
        println!("\nAGENT-SPECIFIC DOCUMENTATION");
        println!("=============================");
        println!();
        println!("Claude Code:");
        println!("  - Requires ANTHROPIC_API_KEY or ~/.claude/.credentials.json");
        println!("  - Supports MCP server integration");
        println!("  - Best for complex reasoning tasks");
        println!();
        println!("Gemini:");
        println!("  - Requires GEMINI_API_KEY");
        println!("  - Fast response times");
        println!("  - Good for straightforward tasks");
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sheep_art_not_empty() {
        assert!(!SHEEP_ART.is_empty());
        assert!(SHEEP_ART.contains('_'));
    }

    #[test]
    fn test_manual_content() {
        assert!(MANUAL.contains("HIRSEL"));
        assert!(MANUAL.contains("DESCRIPTION"));
        assert!(MANUAL.contains("hirsel go"));
        assert!(MANUAL.contains("hirsel-worker"));
    }
}
