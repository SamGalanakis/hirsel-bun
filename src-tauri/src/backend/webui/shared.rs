use maud::{html, Markup, PreEscaped, DOCTYPE};
use pulldown_cmark::{html as md_html, Options, Parser};

use crate::backend::icons::icon;
use crate::backend::shepherd_runtime::ShepherdMessageChunk;

fn markdown_to_html(text: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);
    let mut html_output = String::new();
    md_html::push_html(&mut html_output, parser);
    html_output
}

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
                link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Azeret+Mono:wght@400;500;700;800&family=Chivo+Mono:wght@300;400;500;700&display=swap";
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

pub(crate) fn status_dot_class(status: &str) -> &str {
    match status {
        "running" | "working" | "active" | "starting" => "st-active",
        "done" => "st-done",
        "failed" | "error" => "st-failed",
        _ => "",
    }
}

fn parse_message_chunks(chunks_json: &str) -> Vec<ShepherdMessageChunk> {
    serde_json::from_str(chunks_json).unwrap_or_default()
}

enum ConversationFragment {
    Text(String),
    Tool { title: String, status: String },
    Image { label: String },
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
                kind,
                status,
                ..
            } => {
                let is_batch =
                    kind.as_deref() == Some("batch") || title.eq_ignore_ascii_case("batch");
                if is_batch {
                    continue;
                }
                fragments.push(ConversationFragment::Tool { title, status });
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

    // Collect into groups: text/image render individually, consecutive tools collapse
    let mut output: Vec<Markup> = Vec::new();
    let mut tool_run: Vec<(String, String)> = Vec::new();

    let flush_tools = |tools: &mut Vec<(String, String)>, out: &mut Vec<Markup>| {
        if tools.is_empty() {
            return;
        }
        let n = tools.len();
        let label = if n == 1 {
            tools[0].0.to_string()
        } else {
            format!("Used {} tools", n)
        };
        out.push(html! {
            div class="tool-summary" {
                (icon("cpu"))
                (label)
            }
        });
        tools.clear();
    };

    for fragment in fragments {
        match fragment {
            ConversationFragment::Text(content) => {
                flush_tools(&mut tool_run, &mut output);
                output.push(html! {
                    div class="message-body markdown-body" { (PreEscaped(markdown_to_html(&content))) }
                });
            }
            ConversationFragment::Tool { title, status, .. } => {
                tool_run.push((title, status));
            }
            ConversationFragment::Image { label } => {
                flush_tools(&mut tool_run, &mut output);
                output.push(html! {
                    div class="message-attachment" {
                        (icon("package"))
                        span { (label) }
                    }
                });
            }
        }
    }
    flush_tools(&mut tool_run, &mut output);

    html! {
        @for markup in &output {
            (markup)
        }
    }
}
