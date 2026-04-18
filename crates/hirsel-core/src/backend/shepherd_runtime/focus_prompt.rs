//! Dynamic prompt contributor that injects the project's recently-focused
//! nodes into the shepherd's system prompt on every LLM iteration. Because
//! it runs inside `collect_prompt_contributions` (called per
//! `prepare_turn_machine`), the list refreshes after every tool call —
//! so the assistant always sees what the user is currently looking at.

use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{PluginSpec, PromptContribution};

use crate::backend::project_focus::{recent_focus, RecentFocusEntry};

pub(super) fn focus_prompt_plugin_factory(project_id: i64) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "project_recent_focus",
        PluginSpec::new().with_prompt_contributor(Arc::new(move |_ctx| {
            Box::pin(async move {
                let entries = recent_focus(project_id).await.unwrap_or_default();
                Ok(vec![PromptContribution::guidance(
                    "project_recent_focus",
                    "User Focus",
                    render_focus_block(&entries),
                )])
            })
        })),
    ))
}

fn render_focus_block(entries: &[RecentFocusEntry]) -> String {
    if entries.is_empty() {
        return "The user has not focused any canvas nodes yet in this project.".to_string();
    }

    let mut out = String::from(
        "The user has recently focused these canvas nodes (most recent first). \
         Treat them as strong context for what the user cares about right now.\n",
    );
    for entry in entries {
        let label = entry.label.as_deref().unwrap_or("(unlabeled)");
        let ts = if entry.focused_at.is_empty() {
            String::new()
        } else {
            format!(" — focused {}", entry.focused_at)
        };
        out.push_str(&format!(
            "- [{}:{}] {}{}\n",
            entry.kind, entry.node_id, label, ts
        ));
    }
    out
}
