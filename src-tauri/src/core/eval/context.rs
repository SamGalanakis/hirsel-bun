//! Eval context building and prompt generation.

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::{EvalStatus, Files, ProjectMessagesStore, SQLiteState};

/// Context gathered for eval agent.
pub struct EvalContext {
    pub spec: String,
    pub eval_spec: String,
    pub assets_path: PathBuf,
    pub group_chat: String,
    pub previous_failures: Vec<EvalFailure>,
}

/// A previous failed eval's feedback.
pub struct EvalFailure {
    pub eval_name: String,
    pub feedback: String,
}

/// Get the eval prompt template.
fn get_eval_prompt() -> String {
    r#"# Hirsel Eval Mode

You are an eval agent verifying work done by workers.

## Your Task

Verify the code in your current directory matches what was specified.

## How to Submit Your Verdict

You MUST call one of these MCP tools to complete your evaluation:

- `mcp__eval__eval_pass` - Call if all checks pass
- `mcp__eval__eval_fail(feedback)` - Call if any check fails. Include specific feedback.

Your evaluation is NOT complete until you call one of these tools.

## Guidelines

- Be thorough but focused on the spec
- Don't modify any code - you are read-only
- If a check is ambiguous, fail with clear explanation
- Be specific about what failed and how to fix it
- You have git access for viewing history (git log, git diff, git status) and running hooks (pre-commit)
- Do NOT commit or modify git state - only use it for inspection

## Feedback Format (for eval_fail)

```
Checks:
- [PASS] Check 1 description
- [FAIL] Check 2 description: explanation of failure

To fix: Specific actionable instructions
```
"#
    .to_string()
}

/// Build eval context from run state.
pub async fn build_eval_context(files: &Files, state: &SQLiteState) -> EvalContext {
    // Read spec - warn if missing (eval may fail without it)
    let spec = match fs::read_to_string(files.spec()) {
        Ok(content) => content,
        Err(e) => {
            tracing::warn!(
                "Failed to read spec file '{}': {} - eval may lack context",
                files.spec().display(),
                e
            );
            String::new()
        }
    };

    // Read eval spec - warn if missing (eval criteria unclear)
    let eval_spec = match fs::read_to_string(files.eval_spec()) {
        Ok(content) => content,
        Err(e) => {
            tracing::warn!(
                "Failed to read eval spec '{}': {} - eval criteria may be unclear",
                files.eval_spec().display(),
                e
            );
            String::new()
        }
    };

    // Get assets path
    let assets_path = files.assets();

    // Get group chat messages from project messages
    let group_chat = match (state.get_project_id().await, state.get_route_id().await) {
        (Ok(Some(project_id)), Ok(route_id)) => match ProjectMessagesStore::open().await {
            Ok(store) => match store
                .get_messages(project_id, route_id, "chat", Some(500))
                .await
            {
                Ok(msgs) => format_project_messages(&msgs),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        },
        _ => String::new(),
    };

    // Get previous failed evals
    let previous_failures = match state.get_evals(100).await {
        Ok(evals) => evals
            .iter()
            .filter(|e| e.status == EvalStatus::Failed)
            .filter_map(|e| {
                Some(EvalFailure {
                    eval_name: e.eval_name.clone()?,
                    feedback: e.feedback.clone()?,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    EvalContext {
        spec,
        eval_spec,
        assets_path,
        group_chat,
        previous_failures,
    }
}

/// Format project messages for prompt inclusion.
fn format_project_messages(msgs: &[crate::core::ProjectMessage]) -> String {
    msgs.iter()
        .map(|m| format!("[{}] {}: {}", m.timestamp, m.sender, m.content))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Copy directory recursively.
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let name = match path.file_name() {
            Some(n) => n,
            None => continue,
        };

        let dst_path = dst.join(name);
        if path.is_dir() {
            copy_dir_all(&path, &dst_path)?;
        } else {
            fs::copy(&path, &dst_path)?;
        }
    }
    Ok(())
}

/// Build full eval prompt with context.
pub fn build_eval_prompt(ctx: &EvalContext) -> String {
    let mut prompt = get_eval_prompt();

    prompt.push_str("\n## Original Spec (what workers were asked to build)\n\n");
    prompt.push_str(&ctx.spec);

    prompt.push_str("\n\n## Eval Specification (what to verify)\n\n");
    prompt.push_str(&ctx.eval_spec);

    prompt.push_str(&format!(
        "\n\n## Assets Directory\n\n{}\n",
        ctx.assets_path.display()
    ));
    prompt.push_str("Images and files referenced in spec/eval are available here.\n");

    if !ctx.group_chat.is_empty() {
        prompt.push_str("\n\n## Team Discussion\n\n");
        prompt.push_str(&ctx.group_chat);
    }

    if !ctx.previous_failures.is_empty() {
        prompt.push_str("\n\n## Previous Eval Failures\n\n");
        prompt.push_str("These evals have already failed. Learn from their feedback:\n\n");
        for failure in &ctx.previous_failures {
            prompt.push_str(&format!(
                "### {}\n{}\n\n",
                failure.eval_name, failure.feedback
            ));
        }
    }

    prompt.push_str("\n\nBegin your evaluation by examining the code.\n");

    prompt
}
