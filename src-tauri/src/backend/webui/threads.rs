use maud::{html, Markup};

use crate::backend::icons::icon;
use crate::backend::project::Project;
use crate::backend::shepherd_runtime::ShepherdMessageChunk;
use crate::backend::ShepherdChatMessage;

use super::shared::{format_time, render_message_fragments, status_tone};
use super::ThreadPanelState;

#[derive(Debug, Clone)]
pub(crate) struct ThreadPlanStep {
    pub step: String,
    pub status: String,
}

pub(crate) fn thread_plan(history: &[ShepherdChatMessage]) -> Vec<ThreadPlanStep> {
    for message in history.iter().rev() {
        let Ok(chunks) = serde_json::from_str::<Vec<ShepherdMessageChunk>>(&message.chunks_json)
        else {
            continue;
        };
        for chunk in chunks.into_iter().rev() {
            let ShepherdMessageChunk::Tool { input, .. } = chunk else {
                continue;
            };
            let Some(input) = input else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&input) else {
                continue;
            };
            let Some(plan) = json.get("plan").and_then(|value| value.as_array()) else {
                continue;
            };
            let steps = plan
                .iter()
                .filter_map(|item| {
                    Some(ThreadPlanStep {
                        step: item.get("step")?.as_str()?.to_string(),
                        status: item.get("status")?.as_str()?.to_string(),
                    })
                })
                .collect::<Vec<_>>();
            if !steps.is_empty() {
                return steps;
            }
        }
    }
    Vec::new()
}

pub(crate) fn thread_last_message(history: &[ShepherdChatMessage]) -> Option<&ShepherdChatMessage> {
    history
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
}

pub fn render_threads_panel(project: &Project, threads: &[ThreadPanelState]) -> Markup {
    html! {
        section id="threads-panel" class="thread-deck card" {
            header class="thread-deck-header" {
                div class="thread-deck-copy" {
                    h3 { (icon("cpu")) "Threads" }
                    p class="muted" { "Parallel work managed by shepherd" }
                }
                span class="pill muted" { (threads.len()) " thread" @if threads.len() != 1 { "s" } }
            }
            section {
                @if threads.is_empty() {
                    div class="empty-state" {
                        p { "No visible threads yet." }
                        p class="muted" { "Shepherd can answer directly or spin up threads when a separate line of work deserves its own container." }
                    }
                } @else {
                    div class="thread-grid" {
                        @for item in threads {
                            @let thread = &item.thread;
                            @let plan = thread_plan(&item.history);
                            @let completed_steps = plan.iter().filter(|step| step.status == "completed").count();
                            @let has_active_turn = item.activity.has_active_turn;
                            @let live_turn = item.activity.live_turn.as_ref();
                            a href=(format!("/app/projects/{}/threads/{}", project.id, thread.id)) class=(format!("thread-card status-{}", status_tone(&thread.status))) {
                                div class="thread-card-topline" {
                                    p class="thread-card-title" { (&thread.title) }
                                    span class=(format!("pill status-{}", status_tone(&thread.status))) { (&thread.status) }
                                }
                                @if !thread.objective.trim().is_empty() {
                                    p class="thread-card-objective" { (&thread.objective) }
                                }
                                @if !thread.summary.trim().is_empty() {
                                    p class="thread-card-summary" { (&thread.summary) }
                                }
                                @if !plan.is_empty() {
                                    ol class="thread-plan" {
                                        @for step in plan.iter().take(4) {
                                            li class=(format!("thread-plan-step status-{}", status_tone(&step.status))) {
                                                span class="thread-plan-label" { (&step.step) }
                                                span class="thread-plan-status" { (&step.status) }
                                            }
                                        }
                                    }
                                    @if plan.len() > 4 {
                                        p class="thread-card-summary" { "+" (plan.len() - 4) " more" }
                                    }
                                } @else if let Some(live_turn) = live_turn {
                                    div class="thread-card-preview" {
                                        (render_message_fragments(&live_turn.chunks_json))
                                    }
                                } @else if let Some(last_message) = thread_last_message(&item.history) {
                                    div class="thread-card-preview" {
                                        (render_message_fragments(&last_message.chunks_json))
                                    }
                                }
                                div class="thread-card-meta" {
                                    span { (format_time(&thread.last_activity_at)) }
                                    @if let Some(checkout_name) = thread.checkout_name.as_deref() {
                                        span class="pill muted" { "checkout " (checkout_name) }
                                    }
                                    @if !plan.is_empty() {
                                        span class="pill muted" { "plan " (completed_steps) "/" (plan.len()) }
                                    }
                                    @if has_active_turn {
                                        span class="pill status-working" { "thinking" }
                                    }
                                    @if let Some(error) = item.activity.session.as_ref().and_then(|session| session.last_error.as_deref()).filter(|value| !value.trim().is_empty()) {
                                        span class="pill status-failed" { (error) }
                                    }
                                    span class="thread-card-link" { "View" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
