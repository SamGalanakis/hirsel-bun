use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::backend::app::types::UnreadNotificationsResponse;
use crate::backend::icons::icon;
use crate::backend::project::{Project, ProjectSurfaceSnapshot};
use crate::backend::route::Route;
use crate::backend::shepherd_runtime::{ShepherdMessageChunk, ShepherdQueueState};
use crate::backend::{ShepherdChatMessage, ShepherdThread};

const DATASTAR_BUNDLE: &str = "/static/datastar.js";

#[derive(Debug, Clone)]
pub struct ThreadPanelState {
    pub thread: ShepherdThread,
    pub history: Vec<ShepherdChatMessage>,
    pub queue: ShepherdQueueState,
}

fn page_head(title: &str, description: &str) -> Markup {
    html! {
        meta charset="utf-8";
        meta name="viewport" content="width=device-width, initial-scale=1";
        title { (title) " · Hirsel" }
        meta name="description" content=(description);
        meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; img-src 'self' data: https:; font-src https://fonts.gstatic.com; frame-src 'self'; connect-src 'self';";
        link rel="preconnect" href="https://fonts.googleapis.com";
        link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
        link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Azeret+Mono:wght@400;500;700;800&family=Chivo+Mono:wght@300;400;500;700&family=Spectral:wght@400;500;600;700&display=swap";
        link rel="stylesheet" href="/static/webui.css";
        script type="module" src=(DATASTAR_BUNDLE) {}
    }
}

fn app_document(title: &str, description: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" class="dark" {
            head {
                (page_head(title, description))
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

fn status_tone(status: &str) -> &str {
    match status {
        "active" => "working",
        other => other,
    }
}

fn truncate_copy(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn parse_message_chunks(chunks_json: &str) -> Vec<ShepherdMessageChunk> {
    serde_json::from_str(chunks_json).unwrap_or_default()
}

enum ConversationFragment {
    Text(String),
    Tool {
        title: String,
        status: String,
        detail: Option<String>,
    },
    Image {
        label: String,
    },
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
                status,
                input,
                output,
                ..
            } => {
                let detail = output
                    .or(input)
                    .map(|value| truncate_copy(&value, 180))
                    .filter(|value| !value.is_empty());
                fragments.push(ConversationFragment::Tool {
                    title,
                    status,
                    detail,
                });
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
    html! {
        @for fragment in fragments {
            @match fragment {
                ConversationFragment::Text(content) => {
                    pre class="message-body" { (content) }
                }
                ConversationFragment::Tool { title, status, detail } => {
                    article class="message-tool" {
                        div class="message-tool-header" {
                            span class="message-tool-title" {
                                (icon("cpu"))
                                (title)
                            }
                            span class=(format!("pill status-{}", status_tone(&status))) { (status) }
                        }
                        @if let Some(detail) = detail {
                            p class="message-tool-detail" { (detail) }
                        }
                    }
                }
                ConversationFragment::Image { label } => {
                    div class="message-attachment" {
                        (icon("package"))
                        span { (label) }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ThreadPlanStep {
    step: String,
    status: String,
}

fn thread_plan(history: &[ShepherdChatMessage]) -> Vec<ThreadPlanStep> {
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

fn thread_last_message(history: &[ShepherdChatMessage]) -> Option<&ShepherdChatMessage> {
    history
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
}

fn project_focus_frame_src(project_id: i64, surface: &ProjectSurfaceSnapshot) -> String {
    format!(
        "/app/projects/{}/focus?v={}",
        project_id,
        urlencoding::encode(&surface.focus_view.updated_at)
    )
}

pub fn render_project_focus_stage(project: &Project, surface: &ProjectSurfaceSnapshot) -> Markup {
    let source_label = surface.focus_view.source.as_deref().unwrap_or("live");
    let focus_src = project_focus_frame_src(project.id, surface);
    html! {
        section id="focus-panel" class="focus-stage" {
            header class="focus-stage-header" {
                div class="focus-stage-copy" {
                    p class="eyebrow" { "Canvas" }
                    p class="muted" { "Shared project picture" }
                }
                span class="pill muted" { (source_label) }
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

pub fn render_threads_panel(project: &Project, threads: &[ThreadPanelState]) -> Markup {
    html! {
        section id="threads-panel" class="thread-deck card" {
            header class="thread-deck-header" {
                div class="thread-deck-copy" {
                    h3 { (icon("cpu")) "Threads" }
                    p class="muted" { "Visible parallel terminals on isolated checkouts" }
                }
                span class="pill muted" { (threads.len()) }
            }
            section {
                @if threads.is_empty() {
                    div class="empty-state" {
                        p { "No visible threads yet." }
                        p class="muted" { "Shepherd can answer directly or spin up threads when a separate line of work deserves its own checkout." }
                    }
                } @else {
                    div class="thread-grid" {
                        @for item in threads {
                            @let thread = &item.thread;
                            @let plan = thread_plan(&item.history);
                            @let completed_steps = plan.iter().filter(|step| step.status == "completed").count();
                            @let queued = item.queue.items.iter().filter(|entry| entry.status == "pending").count();
                            @let has_active_turn = item.queue.has_active_turn;
                            article class=(format!("thread-card status-{}", status_tone(&thread.status))) {
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
                                    @if queued > 0 {
                                        span class="pill muted" { "queued " (queued) }
                                    }
                                    a
                                        href=(format!("/app/projects/{}/threads/{}", project.id, thread.id))
                                        class="thread-card-link" {
                                        "Inspect"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Chat Panel ──

pub fn render_chat_panel(
    project_id: i64,
    threads: &[ThreadPanelState],
    history: &[ShepherdChatMessage],
    queue: &ShepherdQueueState,
) -> Markup {
    let active_threads = threads
        .iter()
        .filter(|item| !matches!(item.thread.status.as_str(), "done"))
        .collect::<Vec<_>>();

    html! {
        section id="chat-panel" class="chat-panel shepherd-chat-panel" {
            header class="conversation-header" {
                div class="conversation-header-copy" {
                    p class="eyebrow" { "Shepherd" }
                    p class="muted" { "Talk here; threads carry the parallel work." }
                }
            }

            div id="chat-thread" class="chat-thread shepherd-messages-area" {
                @if history.is_empty() && queue.items.is_empty() {
                    div class="chat-empty-state" {
                        p class="eyebrow" { "No shepherd conversation yet" }
                        p class="muted" { "Ask a question, direct the route, or tell shepherd to start and manage threads for separate work." }
                    }
                } @else {
                    @for message in history {
                        @let is_user = message.role == "user";
                        div class=(if is_user { "chat-row user" } else { "chat-row assistant" }) {
                            article class=(if is_user { "chat-message-user" } else { "chat-message-assistant" }) {
                                div class="message-meta" {
                                    span class="message-author" { (if is_user { "you" } else { "shepherd" }) }
                                    span class="message-time" { (format_time(&message.timestamp)) }
                                }
                                (render_message_fragments(&message.chunks_json))
                            }
                        }
                    }

                    @for item in queue.items.iter().filter(|q| q.status != "working") {
                        div class="chat-row user" {
                            article class="chat-message-user pending" {
                                div class="message-meta" {
                                    span class="message-author" { "Queued" }
                                    span class="message-time" { (format_time(&item.created_at)) }
                                }
                                (render_message_fragments(&item.chunks_json))
                                span class=(if item.status == "failed" { "message-status status-failed" } else { "message-status" }) {
                                    @if item.status == "failed" { "failed" } @else { "queued" }
                                }
                                @if let Some(error) = &item.error {
                                    p class="text-destructive" { (error) }
                                }
                            }
                        }
                    }
                }
            }
            form
                id=(format!("chat-send-form-{}", project_id))
                class="chat-input"
                data-signals:chat-draft="''"
                data-signals:chat-error="''"
                data-signals:chat-sending="false"
                data-indicator:chat-sending
                data-on:submit__prevent=(format!(
                    "if (!$chatDraft.trim()) return; @post('/app/projects/{}/chat/send', {{contentType: 'form', selector: '#chat-send-form-{}'}}); $chatDraft = ''",
                    project_id,
                    project_id
                )) {
                @if queue.has_active_turn || !queue.items.is_empty() || !active_threads.is_empty() {
                    div class="chat-status-bar" {
                        @if queue.has_active_turn {
                            span class="pill status-working" { "Working" }
                        }
                        @if !queue.items.is_empty() {
                            span class="pill" { "Queued " (queue.items.len()) }
                        }
                        @if !active_threads.is_empty() {
                            span class="pill muted" { (active_threads.len()) " active threads" }
                        }
                    }
                }
                p class="text-destructive" data-show="$chatError" data-text="$chatError" {}
                div class="chat-input-wrapper" {
                    input
                        type="text"
                        name="content"
                        placeholder="Ask shepherd to answer, plan, or manage threads..."
                        autocomplete="off"
                        data-bind:chat-draft {}
                    button
                        type="submit"
                        class="btn btn-sm chat-send-btn"
                        data-attr:disabled="$chatSending || !$chatDraft.trim()" {
                        (icon("send"))
                        "Send"
                    }
                }
            }
        }
    }
}

// ── Connect Page ──

pub fn render_connect_page(error: Option<&str>, return_to: Option<&str>) -> Markup {
    app_document(
        "Connect",
        "Connect to a Hirsel backend",
        html! {
            main class="connect-page" {
                section class="card connect-card" {
                    header {
                        h2 { "Connect to Hirsel" }
                        p { "Enter the backend API key to open this Hirsel server." }
                    }
                    section {
                        @if let Some(error) = error {
                            div class="alert alert-destructive" {
                                (icon("x"))
                                strong { "Error" }
                                section { p { (error) } }
                            }
                        }
                        form action="/connect/session" method="post" {
                            input type="hidden" name="return_to" value=(return_to.unwrap_or("/app"));
                            div class="field" {
                                label for="api-key" { "API key" }
                                input id="api-key" type="password" name="api_key" autocomplete="current-password" required;
                            }
                            button type="submit" class="btn btn-full" {
                                (icon("key"))
                                "Open Hirsel"
                            }
                        }
                    }
                }
            }
        },
    )
}

// ── Empty Projects / Sidebar ──

pub fn render_empty_projects_page(
    projects: &[Project],
    create_error: Option<&str>,
    draft_name: Option<&str>,
    draft_repo_url: Option<&str>,
    draft_branch: Option<&str>,
    confirm_create_branch: bool,
    confirm_base_branch: Option<&str>,
) -> Markup {
    let has_projects = !projects.is_empty();
    let name_value = draft_name.unwrap_or_default();
    let repo_value = draft_repo_url.unwrap_or_default();
    let branch_value = draft_branch.unwrap_or("main");
    app_document(
        "Projects",
        "Hirsel project workspace",
        html! {
            main class="welcome-page" {
                // Centered content
                div class="welcome-container" {
                    // Architectural grid motif
                    svg class="welcome-glyph" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="0.6" {
                        rect x="4" y="4" width="40" height="40" {}
                        line x1="4" y1="24" x2="44" y2="24" {}
                        line x1="24" y1="4" x2="24" y2="44" {}
                        rect x="14" y="14" width="20" height="20" opacity="0.25" {}
                    }
                    p class="welcome-brand" { "HIRSEL" }

                    @if has_projects {
                        // Project list + create form
                        div class="welcome-projects" {
                            p class="eyebrow" { "Your projects" }
                            nav class="welcome-project-list" {
                                @for project in projects {
                                    a href=(format!("/app/projects/{}", project.id)) class="welcome-project-item" {
                                        (icon("folder"))
                                        span { (&project.name) }
                                        (icon("arrow-left"))
                                    }
                                }
                            }
                        }
                        hr {}
                    }

                    // Create project form OR branch confirmation
                    div class="welcome-create" {
                        @if confirm_create_branch {
                            // ── Branch confirmation (replaces the form) ──
                            h1 class="welcome-headline" { "Initialize repository?" }
                            p class="muted" {
                                @if let Some(base_branch) = confirm_base_branch.filter(|value| !value.trim().is_empty()) {
                                    "Branch "
                                    code { (branch_value) }
                                    " doesn't exist yet. It will be created from "
                                    code { (base_branch) }
                                    "."
                                } @else {
                                    "This repository is empty. Branch "
                                    code { (branch_value) }
                                    " will be created with an initial commit."
                                }
                            }
                            div class="confirm-actions" {
                                form action="/app/projects/confirm-create" method="post" {
                                    input type="hidden" name="name" value=(name_value);
                                    input type="hidden" name="repo_url" value=(repo_value);
                                    input type="hidden" name="branch" value=(branch_value);
                                    @if let Some(base_branch) = confirm_base_branch.filter(|value| !value.trim().is_empty()) {
                                        input type="hidden" name="base_branch" value=(base_branch);
                                    }
                                    button type="submit" class="btn btn-primary btn-full" {
                                        (icon("plus"))
                                        "Create " (name_value) " on " (branch_value)
                                    }
                                }
                                a href="/app" class="btn btn-ghost btn-full" {
                                    "Cancel"
                                }
                            }
                        } @else {
                            // ── Normal create form ──
                            @if !has_projects {
                                h1 class="welcome-headline" { "Point Hirsel at a repository" }
                                p class="muted" {
                                    "Create a project to start orchestrating work."
                                }
                            } @else {
                                p class="eyebrow" { "New project" }
                            }
                            @if let Some(error) = create_error {
                                @if !error.trim().is_empty() {
                                    div class="settings-inline-alert" {
                                        p class="text-destructive" { (error) }
                                    }
                                }
                            }
                        form
                            action="/app/projects"
                            method="post"
                            class="welcome-form"
                            data-signals:repo-url="''"
                            data-signals:project-name="''"
                            data-computed:repo-base="$repoUrl.trim().replace(/\\/+$/, '').split('/').pop()?.replace(/\\.git$/, '') || ''"
                            data-effect="if (!$projectName.trim() && $repoBase) { $projectName = $repoBase }" {
                            div class="field" {
                                label for="proj-name" { "Name" }
                                input
                                    id="proj-name"
                                    type="text"
                                    name="name"
                                    placeholder="my-project"
                                    value=(name_value)
                                    required
                                    data-bind:project-name;
                            }
                            div class="field" {
                                label for="repo-url" { "Repository" }
                                input
                                    id="repo-url"
                                    type="url"
                                    name="repo_url"
                                    placeholder="https://github.com/owner/repo"
                                    value=(repo_value)
                                    required
                                    data-bind:repo-url;
                            }
                            div class="field" {
                                label for="branch" { "Branch" }
                                input id="branch" type="text" name="branch" value=(branch_value);
                            }
                            button type="submit" class="btn btn-primary btn-full" {
                                (icon("plus"))
                                "Create project"
                            }
                        }
                        } // end @else (non-confirm)
                    }

                    // Footer link
                    div class="welcome-footer" {
                        a href="/app/settings" class="btn btn-ghost" {
                            (icon("settings"))
                            "Backend settings"
                        }
                    }
                }
            }
        },
    )
}

// ── Project Page (Main Workspace) ──

pub fn render_project_page(
    projects: &[Project],
    project: &Project,
    route: &Route,
    surface: &ProjectSurfaceSnapshot,
    threads: &[ThreadPanelState],
    history: &[ShepherdChatMessage],
    queue: &ShepherdQueueState,
    notifications: &UnreadNotificationsResponse,
) -> Markup {
    let route_status = surface
        .routes
        .iter()
        .find(|item| item.route_id == route.id)
        .map(|item| item.status.as_str())
        .unwrap_or("idle");
    let stream_url = format!("/app/projects/{}/stream", project.id);

    app_document(
        &project.name,
        "Hirsel project workspace",
        html! {
            main class="app-shell" data-signals:project-picker-open="false" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}

                // ── Titlebar ──
                header class="titlebar" {
                    div class="titlebar-left" {
                        a href="/app" class="brandmark" { "HIRSEL" }
                        div class="title-divider" {}
                        div class="project-picker" {
                            button type="button" class="btn btn-sm btn-ghost project-chip" data-on:click="$projectPickerOpen = !$projectPickerOpen" {
                                (icon("folder"))
                                (&project.name)
                                (icon("chevron-down"))
                            }
                            // Dropdown
                            div class="project-dropdown" data-show="$projectPickerOpen" {
                                @for p in projects {
                                    @if p.id == project.id {
                                        span class="project-dropdown-item current" {
                                            (icon("folder"))
                                            (&p.name)
                                            (icon("check"))
                                        }
                                    } @else {
                                        a href=(format!("/app/projects/{}", p.id)) class="project-dropdown-item" {
                                            (icon("folder"))
                                            (&p.name)
                                        }
                                    }
                                }
                                hr {}
                                a href="/app" class="project-dropdown-item" {
                                    (icon("plus"))
                                    "New project"
                                }
                            }
                            a href=(format!("/app/projects/{}/settings", project.id)) class="btn-icon" data-tooltip="Project settings" {
                                (icon("settings"))
                            }
                        }
                    }
                    div class="titlebar-right" {
                        @if !notifications.notifications.is_empty() {
                            span class="pill notification-pill" {
                                (icon("bell"))
                                (notifications.notifications.len())
                            }
                        }
                        a href="/app/settings" class="btn-icon" data-tooltip="Settings" {
                            (icon("settings"))
                        }
                    }
                }

                // ── Workbench ──
                section class="workbench" {
                    section class="surface-stack" {
                        header class="surface-toolbar" {
                            div class="route-strip" {
                                span class="pill route-pill" {
                                    (icon("git-branch"))
                                    (&route.name)
                                    @if route_status != "idle" {
                                        " · "
                                        (route_status)
                                    }
                                }
                            }
                            div class="thread-toolbar-actions" {
                                form action=(format!("/app/projects/{}/routes", project.id)) method="post" class="inline-form" {
                                    input type="text" name="name" placeholder="New route name" required;
                                    button type="submit" class="btn btn-sm btn-ghost" {
                                        (icon("copy-plus"))
                                        "Fork route"
                                    }
                                }
                                @if surface.routes.len() > 1 {
                                    form action={ "/app/projects/" (project.id) "/routes/" (route.id) "/archive" } method="post" {
                                        button type="submit" class="btn btn-sm btn-ghost btn-danger" {
                                            (icon("archive"))
                                            "Archive route"
                                        }
                                    }
                                }
                            }
                        }

                        (render_project_focus_stage(project, surface))
                        (render_threads_panel(project, threads))
                    }

                    // ── Chat Rail ──
                    aside class="chat-rail" {
                        (render_chat_panel(project.id, threads, history, queue))
                    }
                }
            }
        },
    )
}

// ── Settings Page ──

pub fn render_settings_page(
    provider: &str,
    openrouter_base_url: Option<&str>,
    openrouter_masked: Option<&str>,
    codex_connected: bool,
    tavily_masked: Option<&str>,
    codex_state: Option<(&str, &str, &str)>,
    setup_required: bool,
    setup_error: Option<&str>,
) -> Markup {
    let page_title = if setup_required {
        "Choose provider"
    } else {
        "Backend settings"
    };
    let page_description = if setup_required {
        "Choose and connect an LLM provider"
    } else {
        "Configure backend providers and services"
    };

    app_document(
        page_title,
        page_description,
        html! {
            main class="shell shell-single" {
                section class="main-panel main-panel-narrow" data-signals:provider-choice=(format!("'{}'", provider)) {

                    // ── Header ──
                    div class="panel-header" {
                        div {
                            @if setup_required {
                                p class="eyebrow" { "LLM provider" }
                                h1 { "Choose how Hirsel should think" }
                                p class="muted" {
                                    "Pick a provider, then finish that provider's setup before opening projects."
                                }
                            } @else {
                                p class="eyebrow" { "Backend" }
                                h1 { "Settings" }
                            }
                        }
                        div id="settings-header-action" {
                            @if !setup_required {
                                a href="/app" class="btn btn-ghost" {
                                    (icon("arrow-left"))
                                    "Back"
                                }
                            }
                        }
                    }
                    @if let Some(error) = setup_error {
                        @if !error.trim().is_empty() {
                            div id="settings-setup-alert" class="settings-inline-alert" {
                                p class="text-destructive" { (error) }
                            }
                        } @else {
                            div id="settings-setup-alert" {}
                        }
                    } @else {
                        div id="settings-setup-alert" {}
                    }

                    // ── LLM Provider ──
                    article class="card" {
                        header {
                            h3 { (icon("cpu")) "LLM Provider" }
                        }
                        section {
                            div class="provider-switch" {
                                button
                                    type="button"
                                    class="provider-option"
                                    data-class:active="$providerChoice === 'codex'"
                                    data-on:click="$providerChoice = 'codex'" {
                                    div class="provider-option-title" { "Codex" }
                                    p class="muted" { "OpenAI account connection for Hirsel." }
                                }
                                button
                                    type="button"
                                    class="provider-option"
                                    data-class:active="$providerChoice === 'openrouter'"
                                    data-on:click="$providerChoice = 'openrouter'" {
                                    div class="provider-option-title" { "OpenRouter" }
                                    p class="muted" { "API key access with optional custom base URL." }
                                }
                            }
                        }
                    }

                    // ── Codex ──
                    article class="card" id="codex-status" data-show="$providerChoice === 'codex'" {
                        header {
                            h3 { (icon("key")) "Codex" }
                            span data-slot="card-action" {
                                @if codex_connected {
                                    span class="pill status-working" { "Connected" }
                                }
                            }
                        }
                        section {
                            @if codex_connected {
                                p class="muted" { "Codex OAuth is connected. Sessions will use Codex for model inference." }
                                @if provider != "codex" {
                                    form action="/app/settings/llm" method="post" {
                                        input type="hidden" name="provider" value="codex";
                                        button type="submit" class="btn btn-full" {
                                            "Use Codex"
                                        }
                                    }
                                }
                            } @else if let Some((device_auth_id, user_code, verify_url)) = codex_state {
                                div data-init=(format!("@get('/app/settings/codex/stream?device_auth_id={}&user_code={}', {{openWhenHidden: true}})", device_auth_id, user_code)) {}
                                p class="muted" { "Open the verification page, enter the code, and keep this page open." }
                                div class="verification-block" {
                                    div class="field" {
                                        label { "Code" }
                                        div class="verification-value-row" {
                                            span class="code-block" { (user_code) }
                                            button
                                                type="button"
                                                class="btn btn-sm btn-ghost"
                                                data-on:click=(format!("navigator.clipboard.writeText({})", serde_json::to_string(user_code).unwrap_or_else(|_| "\"\"".to_string()))) {
                                                (icon("clipboard"))
                                                "Copy code"
                                            }
                                        }
                                    }
                                    div class="field" {
                                        label { "Verification URL" }
                                        div class="verification-value-row" {
                                            span class="code-block code-block-url" { (verify_url) }
                                            button
                                                type="button"
                                                class="btn btn-sm btn-ghost"
                                                data-on:click=(format!("navigator.clipboard.writeText({})", serde_json::to_string(verify_url).unwrap_or_else(|_| "\"\"".to_string()))) {
                                                (icon("clipboard"))
                                                "Copy URL"
                                            }
                                        }
                                    }
                                }
                                div class="verification-actions" {
                                    button
                                        type="button"
                                        class="btn btn-ghost"
                                        data-on:click=(format!("window.open({}, '_blank', 'noopener,noreferrer')", serde_json::to_string(verify_url).unwrap_or_else(|_| "\"\"".to_string()))) {
                                        (icon("globe"))
                                        "Open Codex verification"
                                    }
                                }
                                p class="eyebrow" { "Waiting for approval…" }
                            } @else {
                                form action="/app/settings/codex/start" method="post" {
                                    button type="submit" class="btn btn-primary btn-full" {
                                        (icon("link"))
                                        "Connect Codex"
                                    }
                                }
                            }
                        }
                    }

                    // ── OpenRouter ──
                    article class="card" data-show="$providerChoice === 'openrouter'" {
                        header {
                            h3 { (icon("key")) "OpenRouter" }
                            @if provider == "openrouter" {
                                span data-slot="card-action" {
                                    span class="pill status-working" { "Active" }
                                }
                            }
                        }
                        section {
                            form action="/app/settings/openrouter" method="post" {
                                div class="field" {
                                    label for="or-base" { "Base URL" }
                                    input id="or-base" type="text" name="openrouter_base_url" value=(openrouter_base_url.unwrap_or("")) placeholder="https://openrouter.ai/api/v1";
                                }
                                div class="field" {
                                    label for="or-key" { "OpenRouter API key" }
                                    @if let Some(masked) = openrouter_masked {
                                        p class="muted" { "Current: " (masked) }
                                    }
                                    input id="or-key" type="password" name="api_key" placeholder="sk-or-...";
                                }
                                button type="submit" class="btn btn-full" {
                                    (icon("save"))
                                    "Use OpenRouter"
                                }
                            }
                        }
                    }

                    @if !setup_required {
                        article class="card" {
                            header {
                                h3 { (icon("key")) "Services" }
                            }
                            section {
                                form action="/app/settings/tavily" method="post" {
                                    div class="field" {
                                        label for="tav-key" { "Tavily API key" }
                                        @if let Some(masked) = tavily_masked {
                                            p class="muted" { "Current: " (masked) }
                                        }
                                        input id="tav-key" type="password" name="api_key" placeholder="tvly-...";
                                    }
                                    button type="submit" class="btn btn-ghost" {
                                        (icon("save"))
                                        "Save Tavily key"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

// ── Project Settings ──

pub fn render_project_settings_page(project: &Project, route: &Route) -> Markup {
    app_document(
        "Project settings",
        "Configure project and route defaults",
        html! {
            main class="shell shell-single" {
                section class="main-panel main-panel-narrow" {

                    // ── Header ──
                    div class="panel-header" {
                        div {
                            p class="eyebrow" { (&project.name) }
                            h1 { "Settings" }
                        }
                        a href=(format!("/app/projects/{}", project.id)) class="btn btn-ghost" {
                            (icon("arrow-left"))
                            "Back"
                        }
                    }

                    article class="card" data-signals:confirm-delete="false" {
                        // ── Project ──
                        section {
                            h3 { (icon("folder")) "Project" }
                            form action={ "/app/projects/" (project.id) "/settings/project" } method="post" {
                                div class="field-grid-2" {
                                    div class="field" {
                                        label for="proj-name" { "Name" }
                                        input id="proj-name" type="text" name="name" value=(&project.name);
                                    }
                                    div class="field" {
                                        label for="proj-desc" { "Description" }
                                        input id="proj-desc" type="text" name="description" placeholder="What is this project about?" value=(project.description.as_deref().unwrap_or(""));
                                    }
                                }
                                button type="submit" class="btn btn-full" {
                                    (icon("save"))
                                    "Save project"
                                }
                            }
                        }
                        // ── Route ──
                        section {
                            div class="card-section-header" {
                                h3 { (icon("git-branch")) "Route" }
                                span class="pill" { (&route.name) }
                            }
                            form action={ "/app/projects/" (project.id) "/settings/route" } method="post" {
                                input type="hidden" name="route_id" value=(route.id);
                                div class="field-grid-2" {
                                    div class="field" {
                                        label for="time-limit" { "Time limit (min)" }
                                        input id="time-limit" type="number" min="0" name="time_limit_minutes" value=(route.time_limit_minutes.unwrap_or_default());
                                    }
                                    div class="field" {
                                        label for="target-branch" { "Target branch" }
                                        input id="target-branch" type="text" name="target_branch" placeholder="main" value=(route.target_branch.as_deref().unwrap_or(""));
                                    }
                                }
                                div class="field field-row" {
                                    input id="hitl" type="checkbox" name="human_in_the_loop" checked[route.human_in_the_loop];
                                    label for="hitl" { "Require human in the loop" }
                                }
                                button type="submit" class="btn btn-full" {
                                    (icon("save"))
                                    "Save route"
                                }
                            }
                        }
                        // ── Delete (inline) ──
                        section class="card-danger" {
                            button
                                type="button"
                                class="btn btn-ghost btn-danger"

                                data-show="!$confirmDelete"
                                data-on:click="$confirmDelete = true" {
                                (icon("trash-2"))
                                "Delete project"
                            }
                            form
                                action={ "/app/projects/" (project.id) "/delete" }
                                method="post"
                                data-show="$confirmDelete"
                                {
                                button type="submit" class="btn btn-danger confirmed" {
                                    "Confirm delete"
                                }
                                button type="button" class="btn btn-ghost" data-on:click="$confirmDelete = false" {
                                    "Cancel"
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

// ── Thread Detail ──

pub fn render_thread_detail_page(
    project: &Project,
    route: &Route,
    item: &ThreadPanelState,
) -> Markup {
    let thread = &item.thread;
    let stream_url = format!("/app/projects/{}/threads/{}/stream", project.id, thread.id);
    let plan = thread_plan(&item.history);
    app_document(
        &format!("{} · {}", route.name, thread.title),
        "Thread transcript",
        html! {
            main class="shell shell-single" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}
                section class="main-panel main-panel-wide" {
                    div class="panel-header" {
                        div {
                            h1 { (icon("cpu")) (&thread.title) }
                            p class="muted" {
                                "Route " (&route.name)
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
                                p class="eyebrow" { "Isolated checkout · " (checkout_name) }
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

                    article class="card" {
                        header {
                            h3 { (icon("message-square")) "Transcript" }
                            span data-slot="card-action" {
                                @if item.queue.has_active_turn {
                                    span class="pill status-working" { "thinking" }
                                }
                            }
                        }
                        section {
                            div id="thread-transcript" class="thread-transcript" {
                                @for message in &item.history {
                                    @let is_user = message.role == "user";
                                    div class=(if is_user { "chat-row user" } else { "chat-row assistant" }) {
                                        article class=(if is_user { "chat-message-user" } else { "chat-message-assistant" }) {
                                            div class="message-meta" {
                                                span class="message-author" { (if is_user { "shepherd" } else { "thread" }) }
                                                span class="message-time" { (format_time(&message.timestamp)) }
                                            }
                                            (render_message_fragments(&message.chunks_json))
                                        }
                                    }
                                }
                                @for queued in item.queue.items.iter().filter(|entry| entry.status != "working") {
                                    article class="chat-message-user pending" {
                                        div class="message-meta" {
                                            span class="message-author" { "Queued" }
                                            span class="message-time" { (format_time(&queued.created_at)) }
                                        }
                                        (render_message_fragments(&queued.chunks_json))
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub fn render_focus_document(html_doc: &str) -> Markup {
    PreEscaped(html_doc.to_string())
}
