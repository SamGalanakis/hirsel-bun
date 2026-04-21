//! Non-workspace shell-command instrumentation.
//!
//! Every shell `exec_command` runs through this module's hook. If the
//! command's effective cwd or argv touches paths outside the project's
//! registered workspace roots, the hook appends a line to a per-scope
//! audit log. Not enforcement — just a visible trail so the user knows
//! "thread X ran `brew install foo`".

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{PluginDirective, PluginSpec};
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

/// Register a shepherd-wide plugin that logs non-workspace shell activity.
/// The resulting plugin attaches a `before_tool_call` hook which fires for
/// every `exec_command` invocation.
pub fn audit_plugin_factory(project_id: Option<i64>) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "nonws_audit",
        PluginSpec::new().with_before_tool_call(Arc::new(move |ctx| {
            let project_id = project_id;
            Box::pin(async move {
                if ctx.tool_name != "exec_command" {
                    return Ok(Vec::<PluginDirective>::new());
                }
                if !audit_enabled().await {
                    return Ok(Vec::<PluginDirective>::new());
                }
                let session_id = ctx.session_id.clone();
                let args = ctx.args.clone();
                if let Err(error) = audit_one(project_id, &session_id, &args).await {
                    tracing::debug!(%error, "nonws audit skipped");
                }
                Ok(Vec::<PluginDirective>::new())
            })
        })),
    ))
}

async fn audit_enabled() -> bool {
    RuntimeSettings::get_or(
        keys::WORKSPACE_NONWS_INSTRUMENTATION_ENABLED,
        Defaults::WORKSPACE_NONWS_INSTRUMENTATION_ENABLED,
    )
    .await
}

async fn audit_one(project_id: Option<i64>, session_id: &str, args: &Value) -> Result<(), String> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("cmd").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .or_else(|| {
            args.get("argv").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        })
        .unwrap_or_else(|| "(unknown)".to_string());

    let cwd = args.get("cwd").and_then(|v| v.as_str()).map(PathBuf::from);

    let roots = match project_id {
        Some(pid) => workspace_roots(pid).await,
        None => Vec::new(),
    };
    let escaped_cwd = match &cwd {
        Some(path) => !path_is_inside_any(path, &roots),
        None => false,
    };
    // Heuristic: scan tokens for common global-state mutators.
    let escape_hint = looks_like_global_mutator(&command);

    if !escaped_cwd && !escape_hint && !roots.is_empty() {
        // In-workspace benign command. Don't log — keep the audit file
        // focused on actually-interesting escapes.
        return Ok(());
    }

    let line = format!(
        "{ts}\t{sess}\tescaped_cwd={ecw}\tescape_hint={eh}\tcwd={cwd}\tcommand={cmd}\n",
        ts = crate::backend::db::utc_now(),
        sess = session_id,
        ecw = escaped_cwd,
        eh = escape_hint,
        cwd = cwd
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".into()),
        cmd = command.replace(['\t', '\n'], " "),
    );

    let path = audit_log_path(session_id).await;
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let mut file = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(f) => f,
        Err(error) => return Err(format!("open audit log {}: {error}", path.display())),
    };
    file.write_all(line.as_bytes())
        .await
        .map_err(|e| format!("write audit log: {e}"))?;
    Ok(())
}

async fn audit_log_path(session_id: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".hirsel/audit");
    let safe_session = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    base.join(format!("{safe_session}.log"))
}

async fn workspace_roots(project_id: i64) -> Vec<PathBuf> {
    let store = match crate::backend::ProjectStore::open().await {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let project = match store.get_project(project_id).await {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut roots = Vec::new();
    if let Some(cwd) = project.shepherd_cwd.as_deref() {
        if !cwd.trim().is_empty() {
            roots.push(PathBuf::from(cwd));
        }
    }
    for ws in &project.workspaces {
        if let Some(path) = ws.path.as_deref() {
            if !path.trim().is_empty() {
                roots.push(PathBuf::from(path));
            }
        }
    }
    // Include thread workspace copies base dir — commands inside a copy
    // aren't considered escaped.
    let copy_base = crate::backend::workspace_copy::resolve_base_dir().await;
    roots.push(copy_base);
    roots
}

fn path_is_inside_any(path: &Path, roots: &[PathBuf]) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    roots.iter().any(|root| {
        let root_canon = root.canonicalize().unwrap_or_else(|_| root.clone());
        canonical.starts_with(&root_canon)
    })
}

fn looks_like_global_mutator(command: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "sudo ",
        "apt ",
        "apt-get ",
        "brew ",
        "dpkg ",
        "yum ",
        "dnf ",
        "pacman ",
        "pip install -g",
        "pip install --user",
        "pip3 install -g",
        "pip3 install --user",
        "npm install -g",
        "npm i -g",
        "cargo install",
        "gem install",
        "go install",
        "curl ",
        "wget ",
        "systemctl ",
        "launchctl ",
    ];
    let trimmed = command.trim_start();
    PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix) || trimmed == prefix.trim_end())
}
