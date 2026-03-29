use maud::{html, Markup};

use crate::backend::icons::icon;
use crate::backend::project::Project;

use super::conversation::render_conversation_panel;
use super::shared::status_tone;
use super::threads::thread_plan;
use super::ThreadPanelState;

pub fn render_thread_detail_main(project: &Project, item: &ThreadPanelState) -> Markup {
    let thread = &item.thread;
    let plan = thread_plan(&item.history);
    let form_id = format!("thread-chat-send-form-{}", thread.id);
    let form_action = format!(
        "/app/projects/{}/threads/{}/chat/send",
        project.id, thread.id
    );

    html! {
        section id="thread-detail-main" class="main-panel main-panel-wide" {
            div class="panel-header" {
                div {
                    h1 { (icon("cpu")) (&thread.title) }
                    p class="muted" {
                        "Project thread"
                        " · "
                        span class=(format!("status-{}", status_tone(&thread.status))) {
                            (&thread.status)
                        }
                    }
                }
                a href=(format!("/app/projects/{}", project.id)) class="btn btn-ghost" {
                    (icon("arrow-left"))
                    "Back"
                }
            }

            article class="card" {
                header {
                    h3 { (icon("sparkles")) "Objective" }
                }
                section {
                    p class="muted" { (&thread.objective) }
                    @if let Some(checkout_name) = thread.checkout_name.as_deref() {
                        p class="eyebrow" { "Thread workspace · " (checkout_name) }
                    }
                    @if !plan.is_empty() {
                        ol class="thread-plan thread-plan-detailed" {
                            @for step in plan {
                                li class=(format!("thread-plan-step status-{}", status_tone(&step.status))) {
                                    span class="thread-plan-label" { (&step.step) }
                                    span class="thread-plan-status" { (&step.status) }
                                }
                            }
                        }
                    }
                }
            }

            (render_conversation_panel(
                "thread-chat-panel",
                "Thread",
                "Inspect the live transcript and send guidance directly to this containerized thread.",
                &item.history,
                &item.activity,
                "input",
                "thread",
                "No thread transcript yet",
                "This thread has not received any guidance or produced any visible output yet.",
                &form_id,
                &form_action,
                "Send guidance to this thread...",
                None,
            ))
        }
    }
}

pub fn render_thread_detail_page(project: &Project, item: &ThreadPanelState) -> Markup {
    let stream_url = format!(
        "/app/projects/{}/threads/{}/stream",
        project.id, item.thread.id
    );
    super::shared::app_document(
        &format!("{} · {}", project.name, item.thread.title),
        "Thread transcript",
        html! {
            main class="shell shell-single" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}
                (render_thread_detail_main(project, item))
            }
        },
    )
}
