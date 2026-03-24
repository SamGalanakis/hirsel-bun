use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::core::api_types::{Worker, WorkerEventResponse};
use crate::core::icons::icon;
use crate::core::project::{Project, ProjectSurfaceSnapshot};
use crate::core::route::Route;
use crate::core::worktree::{WorkItemTree, SYNC_PROJECT_TASK_MARKER, SYNC_PROJECT_TASK_TITLE};
use crate::core::{ShepherdChatMessage, ShepherdEffort};
use crate::gui::commands::shepherd::commands::ShepherdQueueState;
use crate::gui::commands::types::UnreadNotificationsResponse;

const DATASTAR_BUNDLE: &str = "/static/datastar.js";
const MAX_VISIBLE_EFFORTS: usize = 5;

fn page_head(title: &str, description: &str) -> Markup {
    html! {
        meta charset="utf-8";
        meta name="viewport" content="width=device-width, initial-scale=1";
        title { (title) " · Hirsel" }
        meta name="description" content=(description);
        meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; img-src 'self' data: https:; font-src https://fonts.gstatic.com; frame-src 'self'; connect-src 'self';";
        link rel="preconnect" href="https://fonts.googleapis.com";
        link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
        link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Newsreader:ital,opsz,wght@0,6..72,300..700;1,6..72,300..700&family=Space+Grotesk:wght@300;400;500;600&display=swap";
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

fn format_time(timestamp: &str) -> String {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(timestamp) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string();
    }
    String::new()
}

fn render_chat_text(chunks_json: &str) -> String {
    let chunks: Vec<serde_json::Value> = serde_json::from_str(chunks_json).unwrap_or_default();
    chunks
        .into_iter()
        .filter_map(|chunk| {
            if chunk.get("type").and_then(|v| v.as_str()) == Some("text") {
                chunk
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|text| text.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn sync_task<'a>(tree: &'a [WorkItemTree]) -> Option<&'a WorkItemTree> {
    for node in tree {
        if node
            .title
            .trim()
            .eq_ignore_ascii_case(SYNC_PROJECT_TASK_TITLE)
            || node.description.contains(SYNC_PROJECT_TASK_MARKER)
        {
            return Some(node);
        }
        if let Some(child) = sync_task(&node.children) {
            return Some(child);
        }
    }
    None
}

fn split_visible_efforts(efforts: &[ShepherdEffort]) -> (&[ShepherdEffort], &[ShepherdEffort]) {
    if efforts.len() <= MAX_VISIBLE_EFFORTS {
        (efforts, &[])
    } else {
        efforts.split_at(MAX_VISIBLE_EFFORTS)
    }
}

// ── Work Tree ──

pub fn render_work_tree_nodes(nodes: &[WorkItemTree]) -> Markup {
    html! {
        @if nodes.is_empty() {
            div class="empty-card" {
                p { "No work items yet." }
            }
        } @else {
            ul class="tree-list" {
                @for node in nodes {
                    li class="tree-node" {
                        div class="tree-row" {
                            div class="tree-meta" {
                                span class="tree-title" { (&node.title) }
                                span class=(format!("pill status-{}", node.status)) { (&node.status) }
                                @if let Some(profile) = node.capability_profile {
                                    span class="pill muted" { (profile.as_str()) }
                                }
                            }
                            @if !node.description.trim().is_empty() {
                                p class="tree-description" { (&node.description) }
                            }
                        }
                        @if !node.children.is_empty() {
                            (render_work_tree_nodes(&node.children))
                        }
                    }
                }
            }
        }
    }
}

// ── Worker Cards ──

pub fn render_worker_cards(workers: &[Worker]) -> Markup {
    html! {
        @if workers.is_empty() {
            div class="empty-card" {
                p { "No workers are active on this route." }
            }
        } @else {
            div class="worker-grid" {
                @for worker in workers {
                    article class="panel worker-card" {
                        header {
                            h3 { (icon("cpu")) (&worker.name) }
                            span data-slot="card-action" {
                                span class=(format!("pill status-{}", format!("{:?}", worker.status).to_lowercase())) {
                                    (format!("{:?}", worker.status).to_lowercase())
                                }
                            }
                        }
                        @if worker.current_task.is_some() || worker.capability_profile.is_some() {
                            section {
                                @if let Some(task) = &worker.current_task {
                                    p class="muted" { (task) }
                                }
                                @if let Some(profile) = &worker.capability_profile {
                                    p class="eyebrow" { (profile.as_str()) }
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
    efforts: &[ShepherdEffort],
    focused_effort: Option<&ShepherdEffort>,
    history: &[ShepherdChatMessage],
    queue: &ShepherdQueueState,
) -> Markup {
    let (visible_efforts, history_efforts) = split_visible_efforts(efforts);
    html! {
        section id="chat-panel" class="chat-panel shepherd-chat-panel" {
            div class="chat-toolbar" {
                div class="chat-toolbar-left" {
                    (icon("message-square"))
                    h2 class="chat-title" {
                        "Shepherd"
                        @if let Some(effort) = focused_effort {
                            span class="chat-effort-title" { " · " (&effort.title) }
                        }
                    }
                }
                div class="chat-toolbar-right" {
                    @if queue.has_active_turn {
                        span class="pill status-working" { "Working" }
                    }
                    @if !queue.items.is_empty() {
                        span class="pill" { "Queued " (queue.items.len()) }
                    }
                }
            }
            hr role="separator" class="shepherd-header-divider" {}
            @if !visible_efforts.is_empty() {
                div class="effort-strip" {
                    @for effort in visible_efforts {
                        @if focused_effort.map(|item| item.id.as_str()) == Some(effort.id.as_str()) {
                            span class="effort-chip active" {
                                (&effort.title)
                            }
                        } @else {
                            form
                                action=(format!("/app/projects/{}/efforts/{}/focus", project_id, effort.id))
                                method="post" {
                                button type="submit" class="effort-chip" {
                                    span { (&effort.title) }
                                    span class=(format!("pill status-{}", effort.status)) { (&effort.status) }
                                }
                            }
                        }
                    }
                }
            }
            @if !history_efforts.is_empty() {
                details class="effort-history" {
                    summary {
                        "History"
                        span class="pill muted" { (history_efforts.len()) }
                    }
                    div class="effort-history-list" {
                        @for effort in history_efforts {
                            form
                                action=(format!("/app/projects/{}/efforts/{}/focus", project_id, effort.id))
                                method="post" {
                                button type="submit" class="effort-history-item" {
                                    span class="effort-history-title" { (&effort.title) }
                                    span class="effort-history-summary" { (&effort.summary) }
                                }
                            }
                        }
                    }
                }
            }
            div id="chat-thread" class="chat-thread shepherd-messages-area" {
                @if focused_effort.is_none() && history.is_empty() && queue.items.is_empty() {
                    div class="shepherd-empty-state" {
                        p class="eyebrow" { "No focused effort" }
                        p class="muted" { "Send a message to start or route work." }
                    }
                } @else if history.is_empty() && queue.items.is_empty() {
                    div class="shepherd-empty-state" {
                        p class="eyebrow" { "Ready" }
                    }
                } @else {
                    @for message in history {
                        @let is_user = message.role == "user";
                        div class=(if is_user { "chat-row user" } else { "chat-row assistant" }) {
                            article class=(if is_user { "shepherd-message-user" } else { "shepherd-message-assistant" }) {
                                div class="message-meta" {
                                    span { (&message.role) }
                                    span { (format_time(&message.timestamp)) }
                                }
                                pre class="message-body" { (render_chat_text(&message.chunks_json)) }
                            }
                        }
                    }
                    // Only show pending/failed queue items (working items are already in history)
                    @for item in queue.items.iter().filter(|q| q.status != "working") {
                        div class="chat-row user" {
                            article class="shepherd-message-user pending" {
                                div class="message-meta" {
                                    span { "queued" }
                                    span { (format_time(&item.created_at)) }
                                }
                                pre class="message-body" { (render_chat_text(&item.chunks_json)) }
                                @if item.status == "failed" {
                                    p class="eyebrow status-failed" { "Failed" }
                                } @else {
                                    p class="eyebrow" { "Queued" }
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
                class="chat-composer shepherd-input-area"
                data-signals:chat-draft="''"
                data-signals:chat-sending="false"
                data-indicator:chat-sending
                data-on:submit__prevent=(format!(
                    "@post('/app/projects/{}/chat/send', {{contentType: 'form', selector: '#chat-send-form-{}'}})",
                    project_id,
                    project_id
                )) {
                div class="shepherd-input-wrapper" {
                    input
                        type="text"
                        name="content"
                        placeholder="Message Shepherd..."
                        autocomplete="off"
                        data-bind:chat-draft {}
                    button
                        type="submit"
                        class="action-btn sm shepherd-send-btn"
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
                section class="panel connect-card" {
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
                            div class="form-field" {
                                label for="api-key" { "API key" }
                                input id="api-key" type="password" name="api_key" autocomplete="current-password" required;
                            }
                            button type="submit" class="action-btn" style="width:100%; margin-top: 8px;" {
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

pub fn render_empty_projects_page(projects: &[Project]) -> Markup {
    app_document(
        "Projects",
        "Create your first Hirsel project",
        html! {
            main class="shell" {
                aside class="sidebar" {
                    div class="brand" { "HIRSEL" }
                    nav class="project-nav" {
                        @for project in projects {
                            a href=(format!("/app/projects/{}", project.id)) class="project-link" {
                                (icon("folder"))
                                (&project.name)
                            }
                        }
                    }
                    div class="sidebar-actions" {
                        a href="/app/settings" class="action-btn ghost" style="width:100%;" {
                            (icon("settings"))
                            "Backend settings"
                        }
                    }
                }
                section class="main-panel" {
                    article class="panel" style="max-width: 540px;" {
                        header {
                            h2 { "Create project" }
                            p { "Point Hirsel at a repository to get started." }
                        }
                        section {
                            form action="/app/projects" method="post" {
                                div class="form-field" {
                                    label for="proj-name" { "Project name" }
                                    input id="proj-name" type="text" name="name" required;
                                }
                                div class="form-field" {
                                    label for="repo-url" { "Repository URL" }
                                    input id="repo-url" type="url" name="repo_url" placeholder="https://github.com/owner/repo" required;
                                }
                                div class="form-field" {
                                    label for="branch" { "Branch" }
                                    input id="branch" type="text" name="branch" value="main";
                                }
                                button type="submit" class="action-btn" style="width:100%; margin-top: 8px;" {
                                    (icon("plus"))
                                    "Create project"
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

// ── Project Page (Main Workspace) ──

pub fn render_project_page(
    _projects: &[Project],
    project: &Project,
    route: &Route,
    surface: &ProjectSurfaceSnapshot,
    work_tree: &[WorkItemTree],
    workers: &[Worker],
    efforts: &[ShepherdEffort],
    focused_effort: Option<&ShepherdEffort>,
    history: &[ShepherdChatMessage],
    queue: &ShepherdQueueState,
    notifications: &UnreadNotificationsResponse,
) -> Markup {
    let sync_state = sync_task(work_tree)
        .map(|item| item.status.as_str())
        .unwrap_or("idle");
    let route_status = surface
        .routes
        .iter()
        .find(|item| item.route_id == route.id)
        .map(|item| item.status.as_str())
        .unwrap_or("idle");
    let has_focus = !matches!(
        surface.focus_view.source.as_deref(),
        Some("placeholder" | "seed")
    );
    let stream_url = format!("/app/projects/{}/stream", project.id);

    app_document(
        &project.name,
        "Hirsel project workspace",
        html! {
            main class="app-shell" data-signals:machinery-open="false" data-signals:machinery-tab="'work'" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}

                // ── Titlebar ──
                header class="titlebar" {
                    div class="titlebar-left" {
                        a href="/app" class="brandmark" { "HIRSEL" }
                        div class="title-divider" {}
                        div class="project-picker" {
                            a href=(format!("/app/projects/{}", project.id)) class="action-btn sm ghost project-chip active" {
                                (icon("folder"))
                                (&project.name)
                            }
                            a href=(format!("/app/projects/{}/settings", project.id)) class="icon-btn" data-tooltip="Project settings" {
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
                        a href="/app/settings" class="icon-btn" data-tooltip="Settings" {
                            (icon("settings"))
                        }
                    }
                }

                // ── Workbench ──
                section class="workbench" {
                    section class="surface-stack" {

                        // ── Surface Toolbar ──
                        header class="surface-toolbar" {
                            div class="route-strip" {
                                span class="pill route-pill" {
                                    (icon("git-branch"))
                                    (&route.name)
                                    " · "
                                    (route_status)
                                }
                            }
                            button type="button" class="action-btn sm ghost toolbar-toggle" data-on:click="$machineryOpen = !$machineryOpen" {
                                "Machinery"
                            }
                        }

                        // ── Focus Stage ──
                        section id="focus-panel" class="focus-stage" {
                            @if has_focus {
                                iframe
                                    title={ "Project focus for " (&project.name) }
                                    src={ "/app/projects/" (project.id) "/focus" }
                                    class="focus-frame" {}
                            } @else {
                                div class="empty-focus-state" {
                                    svg class="empty-glyph" viewBox="0 0 40 40" fill="none" stroke="currentColor" stroke-width="0.75" {
                                        rect x="4" y="4" width="32" height="32" {}
                                        line x1="4" y1="20" x2="36" y2="20" {}
                                        line x1="20" y1="4" x2="20" y2="36" {}
                                        rect x="12" y="12" width="16" height="16" opacity="0.35" {}
                                    }
                                    p class="eyebrow" { "Awaiting project focus" }
                                    @if sync_state == "working" {
                                        p class="muted" { "Hirsel is surveying the project in the background." }
                                    } @else if sync_state == "failed" {
                                        p class="muted" { "Project sync failed. Retry to build the first project picture." }
                                    } @else {
                                        p class="muted" { "Generate the first project picture when you are ready." }
                                    }
                                    @if sync_state != "working" {
                                        form action={ "/app/projects/" (project.id) "/sync" } method="post" class="empty-actions" {
                                            button type="submit" class="action-btn primary" {
                                                (icon("refresh-cw"))
                                                @if sync_state == "failed" { "Retry sync" } @else { "Sync" }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // ── Machinery Drawer ──
                        section class="machinery-drawer" data-show="$machineryOpen" {
                            div class="machinery-header" {
                                span class="eyebrow" { "Machinery" }
                                div class="machinery-actions" {
                                    form action=(format!("/app/projects/{}/routes", project.id)) method="post" class="inline-form" {
                                        input type="text" name="name" placeholder="New route name" required;
                                        button type="submit" class="action-btn sm ghost" {
                                            (icon("copy-plus"))
                                            "Fork"
                                        }
                                    }
                                    @if surface.routes.len() > 1 {
                                        form action={ "/app/projects/" (project.id) "/routes/" (route.id) "/archive" } method="post" {
                                            button type="submit" class="action-btn sm ghost danger" {
                                                (icon("archive"))
                                                "Archive"
                                            }
                                        }
                                    }
                                }
                            }
                            // Tabs
                            div class="tabs machinery-tabs-container" {
                                div role="tablist" {
                                    button role="tab" type="button"
                                        aria-selected="true"
                                        data-class:aria-selected="$machineryTab === 'work'"
                                        data-on:click="$machineryTab = 'work'" {
                                        "Work"
                                    }
                                    button role="tab" type="button"
                                        data-class:aria-selected="$machineryTab === 'workers'"
                                        data-on:click="$machineryTab = 'workers'" {
                                        "Workers"
                                    }
                                }
                            }
                            div class="machinery-body" {
                                section id="work-panel" class="machinery-panel" data-show="$machineryTab === 'work'" {
                                    (render_work_tree_nodes(work_tree))
                                }
                                section id="workers-panel" class="machinery-panel" data-show="$machineryTab === 'workers'" {
                                    (render_worker_cards(workers))
                                }
                            }
                        }
                    }

                    // ── Chat Rail ──
                    aside class="chat-rail" {
                        (render_chat_panel(project.id, efforts, focused_effort, history, queue))
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
) -> Markup {
    app_document(
        "Backend settings",
        "Configure backend providers and services",
        html! {
            main class="shell shell-single" {
                section class="main-panel main-panel-narrow" {

                    // ── Header ──
                    div class="panel-header" style="margin-bottom: 12px;" {
                        div {
                            p class="eyebrow" { "Backend" }
                            h1 { "Settings" }
                        }
                        a href="/app" class="action-btn ghost" {
                            (icon("arrow-left"))
                            "Back"
                        }
                    }

                    // ── LLM Provider ──
                    article class="panel" {
                        header {
                            h3 { (icon("cpu")) "LLM Provider" }
                        }
                        section {
                            form action="/app/settings/llm" method="post" {
                                div class="form-field" {
                                    label for="provider" { "Provider" }
                                    select id="provider" name="provider" {
                                        option value="codex" selected[provider == "codex"] { "Codex (OpenAI)" }
                                        option value="openrouter" selected[provider == "openrouter"] { "OpenRouter" }
                                    }
                                }
                                div class="form-field" {
                                    label for="or-base" { "OpenRouter base URL" }
                                    input id="or-base" type="text" name="openrouter_base_url" value=(openrouter_base_url.unwrap_or(""));
                                }
                                button type="submit" class="action-btn" style="width:100%;" {
                                    (icon("save"))
                                    "Save provider"
                                }
                            }
                        }
                    }

                    // ── Codex ──
                    article class="panel" id="codex-status" {
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
                            } @else if let Some((device_auth_id, user_code, verify_url)) = codex_state {
                                div data-init=(format!("@get('/app/settings/codex/stream?device_auth_id={}&user_code={}', {{openWhenHidden: true}})", device_auth_id, user_code)) {}
                                p class="muted" { "Open the verification page, enter the code, and keep this page open." }
                                div style="margin: 12px 0;" {
                                    span class="code-block" { (user_code) }
                                }
                                p {
                                    a href=(verify_url) target="_blank" rel="noreferrer" class="action-btn ghost" {
                                        (icon("globe"))
                                        "Open Codex verification"
                                    }
                                }
                                p class="eyebrow" { "Waiting for approval…" }
                            } @else {
                                form action="/app/settings/codex/start" method="post" {
                                    button type="submit" class="action-btn primary" {
                                        (icon("link"))
                                        "Connect Codex"
                                    }
                                }
                            }
                        }
                    }

                    // ── API Keys ──
                    article class="panel" {
                        header {
                            h3 { (icon("key")) "API Keys" }
                        }
                        section {
                            form action="/app/settings/openrouter" method="post" {
                                div class="form-field" {
                                    label for="or-key" { "OpenRouter API key" }
                                    @if let Some(masked) = openrouter_masked {
                                        p class="muted" style="font-size: 12px;" { "Current: " (masked) }
                                    }
                                    input id="or-key" type="password" name="api_key" placeholder="sk-or-...";
                                }
                                button type="submit" class="action-btn" style="width:100%;" {
                                    (icon("save"))
                                    "Save OpenRouter key"
                                }
                            }
                            hr role="separator" style="margin: 16px 0;" {}
                            form action="/app/settings/tavily" method="post" {
                                div class="form-field" {
                                    label for="tav-key" { "Tavily API key" }
                                    @if let Some(masked) = tavily_masked {
                                        p class="muted" style="font-size: 12px;" { "Current: " (masked) }
                                    }
                                    input id="tav-key" type="password" name="api_key" placeholder="tvly-...";
                                }
                                button type="submit" class="action-btn ghost" style="width:100%;" {
                                    (icon("save"))
                                    "Save Tavily key"
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
                    div class="panel-header" style="margin-bottom: 12px;" {
                        div {
                            p class="eyebrow" { "Project" }
                            h1 { "Project settings" }
                        }
                        a href=(format!("/app/projects/{}", project.id)) class="action-btn ghost" {
                            (icon("arrow-left"))
                            "Back"
                        }
                    }

                    // ── Project Card ──
                    article class="panel" {
                        header {
                            h3 { (icon("folder")) "Project" }
                        }
                        section {
                            form action={ "/app/projects/" (project.id) "/settings/project" } method="post" {
                                div class="form-field" {
                                    label for="proj-name" { "Project name" }
                                    input id="proj-name" type="text" name="name" value=(&project.name);
                                }
                                div class="form-field" {
                                    label for="proj-desc" { "Description" }
                                    textarea id="proj-desc" name="description" rows="4" { (project.description.as_deref().unwrap_or("")) }
                                }
                                button type="submit" class="action-btn" style="width:100%;" {
                                    (icon("save"))
                                    "Save project"
                                }
                            }
                        }
                    }

                    // ── Route Card ──
                    article class="panel" {
                        header {
                            h3 { (icon("git-branch")) "Route" }
                            span data-slot="card-action" {
                                span class="pill" { (&route.name) }
                            }
                        }
                        section {
                            form action={ "/app/projects/" (project.id) "/settings/route" } method="post" {
                                input type="hidden" name="route_id" value=(route.id);
                                div class="form-field" {
                                    label for="time-limit" { "Time limit (minutes)" }
                                    input id="time-limit" type="number" min="0" name="time_limit_minutes" value=(route.time_limit_minutes.unwrap_or_default());
                                }
                                div class="form-field" style="flex-direction:row; align-items:center; gap:10px;" {
                                    input id="hitl" type="checkbox" name="human_in_the_loop" role="switch" checked[route.human_in_the_loop];
                                    label for="hitl" { "Require human in the loop" }
                                }
                                div class="form-field" {
                                    label for="target-branch" { "Target branch" }
                                    input id="target-branch" type="text" name="target_branch" value=(route.target_branch.as_deref().unwrap_or(""));
                                }
                                button type="submit" class="action-btn" style="width:100%;" {
                                    (icon("save"))
                                    "Save route"
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

// ── Worker Detail ──

pub fn render_worker_detail_page(
    project: &Project,
    route: &Route,
    worker: &Worker,
    events: &[WorkerEventResponse],
) -> Markup {
    let stream_url = format!(
        "/app/projects/{}/routes/{}/workers/{}/stream",
        project.id, route.id, worker.name
    );
    app_document(
        &format!("{} · {}", route.name, worker.name),
        "Worker output",
        html! {
            main class="shell shell-single" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}
                section class="main-panel main-panel-wide" {

                    // ── Header ──
                    div class="panel-header" style="margin-bottom: 12px;" {
                        div {
                            h1 { (icon("cpu")) (&worker.name) }
                            p class="muted" {
                                "Route " (&route.name)
                                " · "
                                span class=(format!("status-{}", format!("{:?}", worker.status).to_lowercase())) {
                                    (format!("{:?}", worker.status).to_lowercase())
                                }
                            }
                        }
                        a href=(format!("/app/projects/{}", project.id)) class="action-btn ghost" {
                            (icon("arrow-left"))
                            "Back"
                        }
                    }

                    // ── Events ──
                    article class="panel" {
                        section {
                            div id="worker-events" class="worker-events" {
                                @for event in events {
                                    article class="event-row" {
                                        div class="message-meta" {
                                            span class="pill" { (&event.event_type) }
                                            span { (format_time(&event.timestamp)) }
                                        }
                                        @if let Some(content) = &event.content {
                                            pre class="message-body" { (content) }
                                        }
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
