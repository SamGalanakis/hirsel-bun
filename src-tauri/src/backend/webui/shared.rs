use maud::{html, Markup, DOCTYPE};

use crate::backend::icons::icon;
use crate::backend::shepherd_runtime::ShepherdMessageChunk;

const DATASTAR_BUNDLE: &str = "/static/datastar.js";

pub(crate) fn app_document(title: &str, description: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" class="dark" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · Hirsel" }
                meta name="description" content=(description);
                meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; img-src 'self' data: https:; font-src https://fonts.gstatic.com; frame-src 'self'; connect-src 'self';";
                link rel="preconnect" href="https://fonts.googleapis.com";
                link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
                link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Azeret+Mono:wght@400;500;700;800&family=Chivo+Mono:wght@300;400;500;700&family=Spectral:wght@400;500;600;700&display=swap";
                link rel="stylesheet" href="/static/webui.css";
                script type="module" src=(DATASTAR_BUNDLE) {}
            }
            body {
                (body)
            }
        }
    }
}

pub(crate) fn format_time(timestamp: &str) -> String {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(timestamp) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string();
    }
    String::new()
}

pub(crate) fn status_tone(status: &str) -> &str {
    match status {
        "active" => "working",
        other => other,
    }
}

fn truncate_copy(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn parse_message_chunks(chunks_json: &str) -> Vec<ShepherdMessageChunk> {
    serde_json::from_str(chunks_json).unwrap_or_default()
}

enum ConversationFragment {
    Text(String),
    Tool {
        title: String,
        status: String,
        detail: Option<String>,
    },
    Image {
        label: String,
    },
}

fn message_fragments(chunks_json: &str) -> Vec<ConversationFragment> {
    let mut fragments = Vec::new();

    for chunk in parse_message_chunks(chunks_json) {
        match chunk {
            ShepherdMessageChunk::Text { content } => {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    fragments.push(ConversationFragment::Text(trimmed.to_string()));
                }
            }
            ShepherdMessageChunk::Tool {
                title,
                status,
                input,
                output,
                ..
            } => {
                let detail = output
                    .or(input)
                    .map(|value| truncate_copy(&value, 180))
                    .filter(|value| !value.is_empty())
                    .filter(|value| !value.starts_with('{') && !value.starts_with('['));
                fragments.push(ConversationFragment::Tool {
                    title,
                    status,
                    detail,
                });
            }
            ShepherdMessageChunk::Image { name, .. } => {
                fragments.push(ConversationFragment::Image {
                    label: name.unwrap_or_else(|| "Image attachment".to_string()),
                });
            }
            ShepherdMessageChunk::Thinking { .. } => {}
        }
    }

    if fragments.is_empty() {
        fragments.push(ConversationFragment::Text(
            "No visible content.".to_string(),
        ));
    }

    fragments
}

pub(crate) fn render_message_fragments(chunks_json: &str) -> Markup {
    let fragments = message_fragments(chunks_json);
    html! {
        @for fragment in fragments {
            @match fragment {
                ConversationFragment::Text(content) => {
                    pre class="message-body" { (content) }
                }
                ConversationFragment::Tool { title, status, detail } => {
                    article class="message-tool" {
                        div class="message-tool-header" {
                            span class="message-tool-title" {
                                (icon("cpu"))
                                (title)
                            }
                            span class=(format!("pill status-{}", status_tone(&status))) { (status) }
                        }
                        @if let Some(detail) = detail {
                            p class="message-tool-detail" { (detail) }
                        }
                    }
                }
                ConversationFragment::Image { label } => {
                    div class="message-attachment" {
                        (icon("package"))
                        span { (label) }
                    }
                }
            }
        }
    }
}
