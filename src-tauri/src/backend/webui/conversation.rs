use maud::{html, Markup};

use crate::backend::icons::icon;
use crate::backend::shepherd_runtime::ShepherdScopeActivity;
use crate::backend::ShepherdChatMessage;

use super::shared::{format_time, render_message_fragments};
use super::ThreadPanelState;

fn render_conversation_messages(
    history: &[ShepherdChatMessage],
    user_label: &str,
    assistant_label: &str,
) -> Markup {
    html! {
        @for message in history {
            @let is_user = message.role == "user";
            @let is_system = message.role == "system";
            div class=(if is_user { "chat-row user" } else { "chat-row assistant" }) {
                article class=(if is_user { "chat-message-user" } else { "chat-message-assistant" }) {
                    div class="message-meta" {
                        span class="message-author" {
                            (if is_user {
                                user_label
                            } else if is_system {
                                "system"
                            } else {
                                assistant_label
                            })
                        }
                        span class="message-time" { (format_time(&message.timestamp)) }
                    }
                    (render_message_fragments(&message.chunks_json))
                }
            }
        }
    }
}

fn render_live_turn(activity: &ShepherdScopeActivity, assistant_label: &str) -> Markup {
    let Some(live_turn) = activity.live_turn.as_ref() else {
        return html! {};
    };

    html! {
        div class="chat-row assistant" {
            article class="chat-message-assistant pending" {
                div class="message-meta" {
                    span class="message-author" { (assistant_label) }
                    span class="message-time" { (format_time(&live_turn.updated_at)) }
                }
                (render_message_fragments(&live_turn.chunks_json))
            }
        }
    }
}

pub(crate) fn render_conversation_panel(
    panel_id: &str,
    header_eyebrow: &str,
    header_copy: &str,
    history: &[ShepherdChatMessage],
    activity: &ShepherdScopeActivity,
    user_label: &str,
    assistant_label: &str,
    empty_eyebrow: &str,
    empty_text: &str,
    form_id: &str,
    form_action: &str,
    form_placeholder: &str,
    active_threads_label: Option<String>,
) -> Markup {
    let active_threads_label = active_threads_label.filter(|value| !value.trim().is_empty());
    let session_status = activity
        .session
        .as_ref()
        .map(|session| session.status.as_str())
        .unwrap_or("idle");
    let last_error = activity
        .session
        .as_ref()
        .and_then(|session| session.last_error.as_deref())
        .filter(|value| !value.trim().is_empty());
    let input_disabled = activity.has_active_turn;

    html! {
        section id=(panel_id) class="chat-panel shepherd-chat-panel" {
            header class="conversation-header" {
                p class="eyebrow" { (header_eyebrow) }
                @if !header_copy.is_empty() {
                    p class="muted" { (header_copy) }
                }
            }

            div class="chat-thread shepherd-messages-area" {
                @if history.is_empty() && activity.live_turn.is_none() {
                    div class="chat-empty-state" {
                        p class="eyebrow" { (empty_eyebrow) }
                        p class="muted" { (empty_text) }
                    }
                } @else {
                    (render_conversation_messages(history, user_label, assistant_label))
                    (render_live_turn(activity, assistant_label))
                }
            }
            form
                id=(form_id)
                class="chat-input"
                data-signals:chat-draft="''"
                data-signals:chat-error="''"
                data-signals:chat-sending="false"
                data-indicator:chat-sending
                data-on:submit__prevent=(format!(
                    "if (!$chatDraft.trim()) return; @post('{}', {{contentType: 'form', selector: '#{}'}}); $chatDraft = ''",
                    form_action,
                    form_id
                )) {
                @if input_disabled || last_error.is_some() || active_threads_label.is_some() {
                    div class="chat-status-bar" {
                        @if input_disabled {
                            span class="pill status-working" { "Working" }
                        } @else {
                            span class=(format!("pill status-{}", session_status)) { (session_status) }
                        }
                        @if let Some(label) = active_threads_label {
                            span class="pill muted" { (label) }
                        }
                        @if let Some(error) = last_error {
                            span class="pill status-failed" { (icon("x")) (error) }
                        }
                    }
                }
                p class="text-destructive" data-show="$chatError" data-text="$chatError" {}
                div class="chat-input-wrapper" {
                    input
                        type="text"
                        name="content"
                        placeholder=(form_placeholder)
                        autocomplete="off"
                        data-bind:chat-draft
                        data-attr:disabled=(if input_disabled { "true" } else { "null" }) {}
                    button
                        type="submit"
                        class="btn btn-sm chat-send-btn"
                        data-attr:disabled=(if input_disabled {
                            "true"
                        } else {
                            "$chatSending || !$chatDraft.trim()"
                        }) {
                        (icon("send"))
                        "Send"
                    }
                }
            }
        }
    }
}

pub fn render_chat_panel(
    project_id: i64,
    threads: &[ThreadPanelState],
    history: &[ShepherdChatMessage],
    activity: &ShepherdScopeActivity,
) -> Markup {
    let active_threads = threads
        .iter()
        .filter(|item| !matches!(item.thread.status.as_str(), "done"))
        .collect::<Vec<_>>();
    let form_id = format!("chat-send-form-{}", project_id);
    let form_action = format!("/app/projects/{}/chat/send", project_id);
    let active_threads_label = (!active_threads.is_empty()).then(|| {
        let n = active_threads.len();
        if n == 1 {
            "1 active thread".to_string()
        } else {
            format!("{n} active threads")
        }
    });

    render_conversation_panel(
        "chat-panel",
        "Shepherd",
        "",
        history,
        activity,
        "you",
        "shepherd",
        "No shepherd conversation yet",
        "Ask a question about the project, or tell shepherd to start and manage threads for separate work.",
        &form_id,
        &form_action,
        "Ask shepherd to answer, plan, or manage threads...",
        active_threads_label,
    )
}
