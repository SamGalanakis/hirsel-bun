//! Per-iteration prompt injection of relevant unresolved comments.
//!
//! For a thread: find nodes it recently read (via `kg_read`) and fetch
//! unresolved comments on those nodes. Comments are rendered as a
//! compact guidance block so the LM has current structured feedback
//! without being overwhelmed.

use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{PluginSpec, PromptContribution};

use crate::backend::kg_comment::{Comment, CommentStore, CommentTarget};
use crate::backend::kg_read::ReadStore;

/// Register a per-runtime plugin that, on each iteration, reads recent
/// kg_read events for the owning thread and injects unresolved comments
/// on those nodes into the system prompt.
pub(super) fn comment_prompt_plugin_factory(
    project_id: i64,
    thread_id: Option<String>,
) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "comment_prompt",
        PluginSpec::new().with_prompt_contributor(Arc::new(move |_ctx| {
            let project_id = project_id;
            let thread_id = thread_id.clone();
            Box::pin(async move {
                let contributions = collect_contributions(project_id, thread_id.as_deref()).await;
                Ok(contributions)
            })
        })),
    ))
}

async fn collect_contributions(
    project_id: i64,
    thread_id: Option<&str>,
) -> Vec<PromptContribution> {
    let pairs = match thread_id {
        Some(tid) => recent_node_pairs_for_thread(tid, 12).await,
        None => Vec::new(),
    };
    if pairs.is_empty() {
        return Vec::new();
    }
    let comments = match CommentStore::open().await {
        Ok(store) => store.list_for_any(project_id, &pairs, 3).await.ok(),
        Err(_) => None,
    };
    let Some(comments) = comments else {
        return Vec::new();
    };
    if comments.is_empty() {
        return Vec::new();
    }
    vec![PromptContribution::guidance(
        "graph_comments",
        "Relevant Comments",
        render_comments(&comments),
    )]
}

async fn recent_node_pairs_for_thread(thread_id: &str, limit: usize) -> Vec<(String, String)> {
    let store = match ReadStore::open().await {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    match store.recent_for_thread(thread_id, limit).await {
        Ok(rows) => rows
            .into_iter()
            .map(|r| (r.node_kind, r.node_id))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn render_comments(comments: &[Comment]) -> String {
    let mut out = String::from(
        "Unresolved comments on nodes you recently touched. Each line is one reviewer's note.\n",
    );
    for c in comments {
        out.push_str(&format!(
            "- [{}:{}] ({author}) {body}{target}\n",
            c.node_kind,
            c.node_id,
            author = c.author,
            body = c.body.lines().next().unwrap_or(&c.body),
            target = render_target(c.target.as_ref()),
        ));
    }
    out
}

fn render_target(target: Option<&CommentTarget>) -> String {
    match target {
        None => String::new(),
        Some(t) => {
            let prop = t.property.as_deref().unwrap_or("");
            let span = match (t.line_start, t.line_end) {
                (Some(a), Some(b)) => format!(":{a}-{b}"),
                (Some(a), None) => format!(":{a}"),
                _ => String::new(),
            };
            if prop.is_empty() && span.is_empty() {
                String::new()
            } else {
                format!(" [{prop}{span}]")
            }
        }
    }
}
