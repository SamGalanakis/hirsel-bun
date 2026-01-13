//! Improve command - update project memory from learnings
//!
//! Analyzes learnings from hirsel runs and updates project memory files
//! (CLAUDE.md or AGENTS.md) by spawning an AI agent to identify patterns
//! and add rules.

use crate::core::{config, state::SQLiteState, Files};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Execute the improve command
pub fn execute(run_name: Option<&str>, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    // If run_name provided, check it exists
    if let Some(name) = run_name {
        if !config::run_exists(name) {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "not_found",
                    "message": format!("Run '{}' not found", name),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Run '{}' not found", name);
            }
            return Ok(());
        }
    }

    if !json {
        println!("Analyzing learnings...");
    }

    // Collect learnings
    let (learnings, latest_timestamps, project_path) = if let Some(name) = run_name {
        collect_learnings_from_run(name)?
    } else {
        collect_all_learnings()?
    };

    // Check if we have any learnings
    if learnings.is_empty() || learnings.values().all(|msgs| msgs.is_empty()) {
        if json {
            let output = serde_json::json!({
                "success": true,
                "message": "No learnings to process",
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("No learnings to process");
        }
        return Ok(());
    }

    // Check project path
    let project_path = match project_path {
        Some(p) if p.exists() => p,
        Some(p) => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "project_not_found",
                    "message": format!("Project path does not exist: {}", p.display()),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Project path does not exist: {}", p.display());
            }
            return Ok(());
        }
        None => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "no_project",
                    "message": "Could not determine project path",
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Could not determine project path");
            }
            return Ok(());
        }
    };

    // Detect memory file
    let memory_file = detect_memory_file(&project_path);
    let memory_exists = memory_file.exists();
    let memory_content = if memory_exists {
        std::fs::read_to_string(&memory_file).unwrap_or_else(|_| "(Could not read file)".to_string())
    } else {
        "(File does not exist yet)".to_string()
    };

    // Format learnings for prompt
    let learnings_text = format_learnings(&learnings);

    // Get the improve prompt
    let base_prompt = get_improve_prompt();

    // Build context
    let context = format!(
        r#"
## Learnings to Analyze

{}

## Project Memory File

**Path**: {}
**Exists**: {}

**Current Contents**:
```
{}
```

Analyze the learnings above and update {} with any patterns you find.
"#,
        learnings_text,
        memory_file.display(),
        memory_exists,
        memory_content,
        memory_file.file_name().unwrap_or_default().to_string_lossy()
    );

    let full_prompt = format!("{}\n\n{}", base_prompt, context);

    // Spawn improve agent
    let result = run_improve_agent(&project_path, &full_prompt, &memory_file);

    match result {
        Ok(output) => {
            // Update learnings_processed_at timestamps
            for (rn, ts) in &latest_timestamps {
                let run_dir = config::run_dir(rn);
                if run_dir.exists() {
                    let files = Files::new(&run_dir);
                    if let Ok(state) = SQLiteState::new(files.db_path()) {
                        let _ = state.set_learnings_processed_at(ts);
                    }
                }
            }

            if json {
                let output_json = serde_json::json!({
                    "success": true,
                    "message": "Project memory updated",
                    "output": output,
                });
                println!("{}", serde_json::to_string_pretty(&output_json)?);
            } else {
                println!("Project memory updated");
            }
        }
        Err(e) => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "agent_failed",
                    "message": e.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Failed to run improve agent: {}", e);
            }
        }
    }

    Ok(())
}

/// Collect learnings from a specific run
fn collect_learnings_from_run(
    run_name: &str,
) -> Result<(std::collections::HashMap<String, Vec<Learning>>, std::collections::HashMap<String, String>, Option<PathBuf>), Box<dyn std::error::Error>> {
    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    let since = state.get_learnings_processed_at().ok().flatten();
    let project_path = state.get_project_path().ok().flatten().map(PathBuf::from);

    let messages = state.get_messages("learnings", 1000)?;

    // Filter by timestamp if needed
    let filtered: Vec<_> = if let Some(ref since_ts) = since {
        messages.into_iter().filter(|m| m.timestamp > *since_ts).collect()
    } else {
        messages
    };

    let learnings: Vec<Learning> = filtered.into_iter().map(|m| Learning {
        sender: m.sender,
        content: m.content,
        timestamp: m.timestamp,
    }).collect();

    let mut result = std::collections::HashMap::new();
    let mut timestamps = std::collections::HashMap::new();

    if !learnings.is_empty() {
        if let Some(latest) = learnings.iter().map(|l| &l.timestamp).max() {
            timestamps.insert(run_name.to_string(), latest.clone());
        }
        result.insert(run_name.to_string(), learnings);
    }

    Ok((result, timestamps, project_path))
}

/// Collect learnings from all runs
fn collect_all_learnings() -> Result<(std::collections::HashMap<String, Vec<Learning>>, std::collections::HashMap<String, String>, Option<PathBuf>), Box<dyn std::error::Error>> {
    let runs_dir = config::runs_dir();
    if !runs_dir.exists() {
        return Ok((std::collections::HashMap::new(), std::collections::HashMap::new(), None));
    }

    let mut all_learnings = std::collections::HashMap::new();
    let mut timestamps = std::collections::HashMap::new();
    let mut project_path = None;

    for entry in std::fs::read_dir(&runs_dir)? {
        let entry = entry?;
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }

        let run_name = match run_dir.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let files = Files::new(&run_dir);
        let db_path = files.db_path();
        if !db_path.exists() {
            continue;
        }

        if let Ok(state) = SQLiteState::new(db_path) {
            let since = state.get_learnings_processed_at().ok().flatten();

            // Get project path from first run that has one
            if project_path.is_none() {
                if let Ok(Some(pp)) = state.get_project_path() {
                    project_path = Some(PathBuf::from(pp));
                }
            }

            if let Ok(messages) = state.get_messages("learnings", 1000) {
                let filtered: Vec<_> = if let Some(ref since_ts) = since {
                    messages.into_iter().filter(|m| m.timestamp > *since_ts).collect()
                } else {
                    messages
                };

                let learnings: Vec<Learning> = filtered.into_iter().map(|m| Learning {
                    sender: m.sender,
                    content: m.content,
                    timestamp: m.timestamp,
                }).collect();

                if !learnings.is_empty() {
                    if let Some(latest) = learnings.iter().map(|l| &l.timestamp).max() {
                        timestamps.insert(run_name.clone(), latest.clone());
                    }
                    all_learnings.insert(run_name, learnings);
                }
            }
        }
    }

    Ok((all_learnings, timestamps, project_path))
}

/// Learning message
struct Learning {
    sender: String,
    content: String,
    timestamp: String,
}

/// Detect the project memory file (CLAUDE.md or AGENTS.md)
fn detect_memory_file(project_path: &Path) -> PathBuf {
    let claude_md = project_path.join("CLAUDE.md");
    let agents_md = project_path.join("AGENTS.md");

    if claude_md.exists() {
        claude_md
    } else if agents_md.exists() {
        agents_md
    } else {
        agents_md // Default to AGENTS.md
    }
}

/// Format learnings for the prompt
fn format_learnings(learnings: &std::collections::HashMap<String, Vec<Learning>>) -> String {
    if learnings.is_empty() {
        return "(No learnings found)".to_string();
    }

    let mut parts = Vec::new();
    for (run_name, messages) in learnings {
        parts.push(format!("## Run: {}\n", run_name));
        for msg in messages {
            let ts = if msg.timestamp.len() > 16 {
                &msg.timestamp[..16]
            } else {
                &msg.timestamp
            }.replace('T', " ");
            parts.push(format!("**{}** ({}):\n{}\n", msg.sender, ts, msg.content));
        }
        parts.push(String::new());
    }

    parts.join("\n")
}

/// Get the improve prompt
fn get_improve_prompt() -> String {
    r#"# Improve Agent

You analyze learnings from hirsel runs and update project memory (CLAUDE.md or AGENTS.md).

## Your Task

1. Read the learnings messages provided
2. Identify patterns (2+ occurrences = pattern, 3+ = strong pattern)
3. Check existing project memory for rule violations
4. Update project memory with new rules

## Process

### Step 1: Analyze Learnings

Look for:
- **Repeated patterns** - Same insight mentioned multiple times
- **User preferences** - Code style, commit format, testing requirements
- **Project-specific knowledge** - Architecture decisions, file locations, gotchas
- **What worked** - Successful approaches worth remembering
- **What didn't work** - Anti-patterns to avoid

Single observations (1 occurrence) are noted but NOT added to memory.

### Step 2: Check for Rule Violations

If the project memory file exists, check if any learnings indicate violations of existing rules.
These get **highest priority** for strengthening.

### Step 3: Update Project Memory

**Format requirements:**
- One line per rule
- Bullet points (- or *)
- Direct, imperative tone ("use X", "avoid Y", "run Z before...")
- NO explanations or rationale in the file
- Group by category if the file has sections

### Step 4: Report Changes

After updating, report:
- Number of patterns identified
- Rules strengthened (if any)
- New rules added
- File updated

## Important

- **Be selective** - Only add rules that will genuinely help future runs
- **Be concise** - One line per rule, no explanations
- **Patterns matter** - Single observations don't become rules
- **Preserve structure** - If the file has sections, maintain them
- **Don't duplicate** - Check existing rules before adding
"#.to_string()
}

/// Run the improve agent (spawns claude CLI)
fn run_improve_agent(
    project_path: &Path,
    prompt: &str,
    memory_file: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let task = format!(
        "Analyze the learnings and update {}. Report what you changed.",
        memory_file.file_name().unwrap_or_default().to_string_lossy()
    );

    let output = Command::new("claude")
        .args([
            "--system-prompt",
            prompt,
            "--dangerously-skip-permissions",
            "-p",
            &task,
        ])
        .current_dir(project_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Agent exited with code {:?}: {}", output.status.code(), stderr).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_memory_file_default() {
        let temp_dir = std::env::temp_dir();
        let result = detect_memory_file(&temp_dir);
        assert!(result.ends_with("AGENTS.md"));
    }

    #[test]
    fn test_format_learnings_empty() {
        let learnings = std::collections::HashMap::new();
        assert_eq!(format_learnings(&learnings), "(No learnings found)");
    }

    #[test]
    fn test_get_improve_prompt() {
        let prompt = get_improve_prompt();
        assert!(prompt.contains("Improve Agent"));
        assert!(prompt.contains("pattern"));
    }
}
