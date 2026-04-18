use askama::Template;

fn render_template<T: Template>(label: &str, template: &T) -> Result<String, String> {
    template
        .render()
        .map_err(|error| format!("failed to render {label}: {error}"))
}

#[derive(Template)]
#[template(path = "prompts/thread_scope_guidance.txt", escape = "none")]
struct ThreadScopeGuidanceTemplate<'a> {
    title: &'a str,
    thread_id: &'a str,
    focus_line: &'a str,
    workspace_root: &'a str,
}

#[derive(Template)]
#[template(path = "prompts/general_scope_guidance.txt", escape = "none")]
struct GeneralScopeGuidanceTemplate<'a> {
    workspace_root: &'a str,
}

#[derive(Template)]
#[template(path = "prompts/shepherd_scope_guidance.txt", escape = "none")]
struct ShepherdScopeGuidanceTemplate<'a> {
    scope_label: &'a str,
    focus_line: &'a str,
    workspace_root: &'a str,
}

#[derive(Template)]
#[template(path = "prompts/skill_block.txt", escape = "none")]
struct SkillBlockTemplate<'a> {
    name: &'a str,
    path: &'a str,
    instructions: &'a str,
}

#[derive(Template)]
#[template(path = "prompts/workspace_file_status.txt", escape = "none")]
struct WorkspaceFileStatusTemplate<'a> {
    root_id: &'a str,
    path: &'a str,
    status: &'a str,
}

#[derive(Template)]
#[template(path = "prompts/workspace_file_binary.txt", escape = "none")]
struct WorkspaceFileBinaryTemplate<'a> {
    root_id: &'a str,
    path: &'a str,
    mime: &'a str,
}

#[derive(Template)]
#[template(path = "prompts/workspace_file_content.txt", escape = "none")]
struct WorkspaceFileContentTemplate<'a> {
    root_id: &'a str,
    path: &'a str,
    language: &'a str,
    line_start: usize,
    line_end: usize,
    truncated: bool,
    content: &'a str,
}

pub(crate) fn render_thread_scope_guidance(
    title: &str,
    thread_id: &str,
    focus_line: &str,
    workspace_root: &str,
) -> Result<String, String> {
    render_template(
        "thread scope guidance template",
        &ThreadScopeGuidanceTemplate {
            title,
            thread_id,
            focus_line,
            workspace_root,
        },
    )
}

pub(crate) fn render_general_scope_guidance(workspace_root: &str) -> Result<String, String> {
    render_template(
        "general scope guidance template",
        &GeneralScopeGuidanceTemplate { workspace_root },
    )
}

pub(crate) fn render_shepherd_scope_guidance(
    scope_label: &str,
    focus_line: &str,
    workspace_root: &str,
) -> Result<String, String> {
    render_template(
        "shepherd scope guidance template",
        &ShepherdScopeGuidanceTemplate {
            scope_label,
            focus_line,
            workspace_root,
        },
    )
}

pub(crate) fn render_skill_block(
    name: &str,
    path: &str,
    instructions: &str,
) -> Result<String, String> {
    render_template(
        "skill block template",
        &SkillBlockTemplate {
            name,
            path,
            instructions,
        },
    )
}

pub(crate) fn render_workspace_file_status(
    root_id: &str,
    path: &str,
    status: &str,
) -> Result<String, String> {
    render_template(
        "workspace file status template",
        &WorkspaceFileStatusTemplate {
            root_id,
            path,
            status,
        },
    )
}

pub(crate) fn render_workspace_file_binary(
    root_id: &str,
    path: &str,
    mime: &str,
) -> Result<String, String> {
    render_template(
        "workspace file binary template",
        &WorkspaceFileBinaryTemplate {
            root_id,
            path,
            mime,
        },
    )
}

pub(crate) fn render_workspace_file_content(
    root_id: &str,
    path: &str,
    language: &str,
    line_start: usize,
    line_end: usize,
    truncated: bool,
    content: &str,
) -> Result<String, String> {
    render_template(
        "workspace file content template",
        &WorkspaceFileContentTemplate {
            root_id,
            path,
            language,
            line_start,
            line_end,
            truncated,
            content,
        },
    )
}
