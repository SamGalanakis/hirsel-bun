use maud::{html, Markup, PreEscaped};

use crate::backend::project::{Project, ProjectSurfaceSnapshot};

fn project_focus_frame_src(project_id: i64, surface: &ProjectSurfaceSnapshot) -> String {
    format!(
        "/app/projects/{}/focus?v={}",
        project_id,
        urlencoding::encode(&surface.focus_view.updated_at)
    )
}

pub fn render_project_focus_stage(project: &Project, surface: &ProjectSurfaceSnapshot) -> Markup {
    let is_placeholder = surface
        .focus_view
        .source
        .as_deref()
        .unwrap_or("placeholder")
        == "placeholder";

    if is_placeholder {
        // No canvas yet — render nothing
        return html! {
            section id="focus-panel" {}
        };
    }

    let focus_src = project_focus_frame_src(project.id, surface);

    html! {
        section id="focus-panel" class="canvas-strip"
            data-signals:canvas-open="false"
        {
            div class="canvas-strip-bar"
                data-on:click="$canvasOpen = !$canvasOpen" {
                span { "Canvas" }
                span { (if true { "expand" } else { "collapse" }) }
            }
            div class="canvas-content" data-show="$canvasOpen" {
                iframe
                    title={ "Canvas for " (&project.name) }
                    src=(focus_src)
                    class="canvas-frame" {}
            }
        }
    }
}

pub fn render_focus_document(html_doc: &str) -> Markup {
    PreEscaped(html_doc.to_string())
}
