use maud::{html, Markup};

use crate::backend::icons::icon;
use crate::backend::shepherd_runtime::ShepherdScopeActivity;
use crate::backend::ShepherdChatMessage;

use super::shared::{format_time, render_message_fragments};
use super::ThreadPanelState;

fn render_conversation_messages(history: &[ShepherdChatMessage]) -> Markup {
    html! {
        @for message in history {
            @let is_user = message.role == "user";
            @let time = format_time(&message.timestamp);
            div class=(if is_user { "chat-row user" } else { "chat-row assistant" }) {
                article class=(if is_user { "chat-message-user" } else { "chat-message-assistant" }) {
                    @if !time.is_empty() {
                        span class="message-time" { (time) }
                    }
                    (render_message_fragments(&message.chunks_json))
                }
            }
        }
    }
}

fn render_live_turn(activity: &ShepherdScopeActivity) -> Markup {
    let Some(live_turn) = activity.live_turn.as_ref() else {
        return html! {};
    };

    html! {
        div class="chat-row assistant" {
            article class="chat-message-assistant pending" {
                span class="message-time" { (format_time(&live_turn.updated_at)) }
                (render_message_fragments(&live_turn.chunks_json))
            }
        }
    }
}

pub(crate) fn render_conversation_panel(
    panel_id: &str,
    history: &[ShepherdChatMessage],
    activity: &ShepherdScopeActivity,
    form_id: &str,
    form_action: &str,
    form_placeholder: &str,
    stop_action: Option<&str>,
    has_queued: bool,
) -> Markup {
    let is_running = activity.has_active_turn;
    let can_stop = is_running && stop_action.is_some();

    let placeholder = if is_running && !has_queued {
        "Queue a follow-up..."
    } else {
        form_placeholder
    };

    html! {
        section id=(panel_id) class="chat-panel shepherd-chat-panel"
            data-on:keydown__window=[can_stop.then(|| format!(
                "if (event.key === 'Escape') @post('{}')",
                stop_action.unwrap_or_default()
            ))]
        {
            div class="chat-thread shepherd-messages-area" {
                @if history.is_empty() && activity.live_turn.is_none() {
                    div class="chat-empty-state" {
                        "Send a message to get started"
                    }
                } @else {
                    (render_conversation_messages(history))
                    (render_live_turn(activity))
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
                @if is_running || has_queued {
                    div class="chat-status-bar" {
                        @if is_running {
                            span class="chat-status-dot" {}
                            "Working"
                        }
                        @if has_queued {
                            span { " · queued" }
                        }
                    }
                }
                p class="text-destructive" data-show="$chatError" data-text="$chatError" {}
                div class="chat-input-wrapper" {
                    input
                        type="text"
                        name="content"
                        placeholder=(placeholder)
                        autocomplete="off"
                        data-bind:chat-draft {}
                    @if can_stop {
                        button
                            type="button"
                            class="chat-stop-btn"
                            data-on:click__prevent=(format!("@post('{}')", stop_action.unwrap_or_default()))
                        {
                            (icon("square"))
                            "Stop"
                        }
                    } @else {
                        button
                            type="submit"
                            class="btn btn-sm chat-send-btn"
                            data-attr:disabled="$chatSending || !$chatDraft.trim()" {
                            (icon("send"))
                            @if is_running { "Queue" } @else { "Send" }
                        }
                    }
                }
                @if is_running {
                    div class="chat-kb-hint" {
                        kbd { "Esc" } " stop"
                        kbd { "Enter" } " queue"
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
    let scope = crate::backend::shepherd_runtime::ShepherdScope::Project {
        project_id,
        workspace_path: None,
        focus: None,
    };
    let _ = threads; // threads now rendered in sidebar, not here
    let form_id = format!("chat-send-form-{}", project_id);
    let form_action = format!("/app/projects/{}/chat/send", project_id);
    let stop_action = format!("/app/projects/{}/chat/stop", project_id);
    let has_queued = crate::backend::shepherd_runtime::has_queued_turn(&scope);

    render_conversation_panel(
        "chat-panel",
        history,
        activity,
        &form_id,
        &form_action,
        "Message shepherd...",
        Some(&stop_action),
        has_queued,
    )
}
