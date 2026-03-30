use maud::{html, Markup};

use crate::backend::project::Project;

use super::shared::status_dot_class;
use super::ThreadPanelState;

fn thread_one_line_summary(item: &ThreadPanelState) -> String {
    if !item.thread.summary.trim().is_empty() {
        return truncate(&item.thread.summary, 60);
    }
    if !item.thread.objective.trim().is_empty() {
        return truncate(&item.thread.objective, 60);
    }
    String::new()
}

fn truncate(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max).collect();
    out.push_str("...");
    out
}

pub fn render_threads_panel(project: &Project, threads: &[ThreadPanelState]) -> Markup {
    html! {
        section id="threads-panel" class="thread-sidebar" {
            div class="thread-sidebar-header" {
                span { "Threads" }
                @if !threads.is_empty() {
                    span { (threads.len()) }
                }
            }
            @if threads.is_empty() {
                div class="thread-sidebar-empty" {
                    "No threads yet"
                }
            } @else {
                div class="thread-sidebar-list" {
                    @for item in threads {
                        @let thread = &item.thread;
                        @let summary = thread_one_line_summary(item);
                        @let dot = status_dot_class(&thread.status);
                        a href=(format!("/app/projects/{}/threads/{}", project.id, thread.id))
                            class="thread-sidebar-item" {
                            div class="thread-sidebar-topline" {
                                span class="thread-sidebar-title" { (&thread.title) }
                                span class=(format!("thread-sidebar-status {}", dot)) {}
                            }
                            @if !summary.is_empty() {
                                span class="thread-sidebar-summary" { (summary) }
                            }
                        }
                    }
                }
            }
        }
    }
}
