use maud::{html, Markup};

use crate::backend::icons::icon;
use crate::backend::project::Project;

use super::conversation::render_conversation_panel;
use super::shared::status_dot_class;
use super::ThreadPanelState;

pub fn render_thread_detail_main(project: &Project, item: &ThreadPanelState) -> Markup {
    let thread = &item.thread;
    let form_id = format!("thread-chat-send-form-{}", thread.id);
    let form_action = format!(
        "/app/projects/{}/threads/{}/chat/send",
        project.id, thread.id
    );
    let stop_action = format!(
        "/app/projects/{}/threads/{}/chat/stop",
        project.id, thread.id
    );
    let scope = crate::backend::shepherd_runtime::ShepherdScope::Thread {
        project_id: project.id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        workspace_path: thread.workspace_path.clone(),
        focus: None,
    };
    let has_queued = crate::backend::shepherd_runtime::has_queued_turn(&scope);
    let dot = status_dot_class(&thread.status);

    html! {
        section id="thread-detail-main" class="main-panel main-panel-wide" {
            div class="panel-header" {
                div {
                    h1 {
                        (&thread.title)
                        span class=(format!("thread-sidebar-status {}", dot))
                            style="display: inline-block; margin-left: 8px; vertical-align: middle;" {}
                    }
                    @if !thread.objective.trim().is_empty() {
                        p class="muted" style="font-size: 11px;" { (&thread.objective) }
                    }
                }
                a href=(format!("/app/projects/{}", project.id)) class="btn btn-ghost" {
                    (icon("arrow-left"))
                    "Back"
                }
            }

            (render_conversation_panel(
                "thread-chat-panel",
                &item.history,
                &item.activity,
                &form_id,
                &form_action,
                "Send guidance to this thread...",
                Some(&stop_action),
                has_queued,
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
