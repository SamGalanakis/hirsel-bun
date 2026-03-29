use maud::{html, Markup, PreEscaped};

use crate::backend::project::{Project, ProjectSurfaceSnapshot};

use super::shared::format_time;

fn project_focus_frame_src(project_id: i64, surface: &ProjectSurfaceSnapshot) -> String {
    format!(
        "/app/projects/{}/focus?v={}",
        project_id,
        urlencoding::encode(&surface.focus_view.updated_at)
    )
}

pub fn render_project_focus_stage(project: &Project, surface: &ProjectSurfaceSnapshot) -> Markup {
    let source_label = surface.focus_view.source.as_deref().unwrap_or("live");
    let is_placeholder = source_label == "placeholder";
    let focus_src = project_focus_frame_src(project.id, surface);
    let updated_label = format_time(&surface.focus_view.updated_at);

    if is_placeholder {
        return html! {
            section id="focus-panel" class="focus-stage" {
                div class="focus-collapsed-strip" {
                    p class="eyebrow" { "Canvas" }
                    p class="muted" { "Shepherd will populate this as the project develops" }
                }
            }
        };
    }

    html! {
        section id="focus-panel" class="focus-stage" {
            header class="focus-stage-header" {
                p class="eyebrow" { "Canvas" }
                div class="focus-stage-meta" {
                    span class="pill muted" { (source_label) }
                    @if !updated_label.is_empty() {
                        span class="pill muted" { (updated_label) }
                    }
                }
            }
            div class="focus-content" {
                iframe
                    title={ "Project focus for " (&project.name) }
                    src=(focus_src)
                    class="focus-frame" {}
            }
        }
    }
}

pub fn render_focus_document(html_doc: &str) -> Markup {
    PreEscaped(html_doc.to_string())
}
