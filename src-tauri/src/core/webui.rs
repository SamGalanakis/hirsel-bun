use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::core::api_types::{Worker, WorkerEventResponse};
use crate::core::app::types::UnreadNotificationsResponse;
use crate::core::icons::icon;
use crate::core::project::{Project, ProjectSurfaceSnapshot};
use crate::core::route::Route;
use crate::core::shepherd_runtime::ShepherdQueueState;
use crate::core::worktree::WorkItemTree;
use crate::core::{ShepherdChatMessage, ShepherdEffort};

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
            div class="empty-state" {
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

fn worker_status_label(worker: &Worker) -> String {
    format!("{:?}", worker.status).to_lowercase()
}

pub fn render_agent_cards(
    efforts: &[ShepherdEffort],
    focused_effort: Option<&ShepherdEffort>,
    workers: &[Worker],
) -> Markup {
    html! {
        @if efforts.is_empty() && workers.is_empty() {
            div class="empty-state" {
                p { "No agents are active on this route." }
            }
        } @else {
            div class="worker-grid" {
                @for effort in efforts {
                    @let is_focused = focused_effort.map(|item| item.id.as_str()) == Some(effort.id.as_str());
                    article class=(if is_focused { "card worker-card active-agent-card" } else { "card worker-card" }) {
                        header {
                            h3 { (icon("cpu")) (&effort.title) }
                            span data-slot="card-action" {
                                span class=(format!("pill status-{}", effort.status)) {
                                    (&effort.status)
                                }
                            }
                        }
                        section {
                            p class="eyebrow" { "orchestrator" }
                            @if !effort.summary.trim().is_empty() {
                                p class="muted" { (&effort.summary) }
                            } @else {
                                p class="muted" { "Shepherd is coordinating this effort." }
                            }
                        }
                    }
                }
                @for worker in workers {
                    article class="card worker-card" {
                        header {
                            h3 { (icon("cpu")) (&worker.name) }
                            span data-slot="card-action" {
                                @let worker_status = worker_status_label(worker);
                                span class=(format!("pill status-{}", worker_status)) {
                                    (worker_status)
                                }
                            }
                        }
                        section {
                            p class="eyebrow" { "worker" }
                            @if let Some(task) = &worker.current_task {
                                p class="muted" { (task) }
                            }
                            @if let Some(profile) = &worker.capability_profile {
                                p class="muted" { "profile: " (profile.as_str()) }
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
            @if !visible_efforts.is_empty() {
                div class="effort-rail" {
                    p class="effort-strip-label" { "Efforts" }
                    div class="effort-strip" {
                        @for effort in visible_efforts {
                            @if focused_effort.map(|item| item.id.as_str()) == Some(effort.id.as_str()) {
                                span class="effort-chip active" {
                                    span class="effort-chip-title" { (&effort.title) }
                                    span class=(format!("pill status-{}", effort.status)) { (&effort.status) }
                                }
                            } @else {
                                form
                                    action=(format!("/app/projects/{}/efforts/{}/focus", project_id, effort.id))
                                    method="post" {
                                    button type="submit" class="effort-chip" {
                                        span class="effort-chip-title" { (&effort.title) }
                                        span class=(format!("pill status-{}", effort.status)) { (&effort.status) }
                                    }
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
            @if let Some(effort) = focused_effort {
                section class="chat-effort-context" {
                    div class="chat-effort-context-header" {
                        p class="eyebrow" { "Focused effort" }
                        span class=(format!("pill status-{}", effort.status)) { (&effort.status) }
                    }
                    p class="chat-effort-context-title" { (&effort.title) }
                    p class="chat-effort-context-summary muted" { (&effort.summary) }
                }
            }
            div id="chat-thread" class="chat-thread shepherd-messages-area" {
                @if history.is_empty() && queue.items.is_empty() {
                    div class="chat-empty-state" {
                        @if let Some(effort) = focused_effort {
                            p class="eyebrow" { (&effort.title) }
                            p class="muted" { (&effort.summary) }
                            @if effort.status == "active" {
                                p class="muted" { "Shepherd is already working on this effort. Updates will appear here as progress lands." }
                            } @else {
                                p class="muted" { "This effort has no conversation events yet." }
                            }
                        } @else {
                            p class="muted" { "Send a message to get started." }
                        }
                    }
                } @else {
                    @for message in history {
                        @let is_user = message.role == "user";
                        div class=(if is_user { "chat-row user" } else { "chat-row assistant" }) {
                            article class=(if is_user { "chat-message-user" } else { "chat-message-assistant" }) {
                                pre class="message-body" { (render_chat_text(&message.chunks_json)) }
                                span class="message-time" { (format_time(&message.timestamp)) }
                            }
                        }
                    }
                    // Only show pending/failed queue items (working items are already in history)
                    @for item in queue.items.iter().filter(|q| q.status != "working") {
                        div class="chat-row user" {
                            article class="shepherd-message-user pending" {
                                pre class="message-body" { (render_chat_text(&item.chunks_json)) }
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
                class="chat-composer shepherd-input-area"
                data-signals:chat-draft="''"
                data-signals:chat-error="''"
                data-signals:chat-sending="false"
                data-indicator:chat-sending
                data-on:submit__prevent=(format!(
                    "if (!$chatDraft.trim()) return; @post('/app/projects/{}/chat/send', {{contentType: 'form', selector: '#chat-send-form-{}'}}); $chatDraft = ''",
                    project_id,
                    project_id
                )) {
                @if queue.has_active_turn || !queue.items.is_empty() {
                    div class="chat-status-bar" {
                        @if queue.has_active_turn {
                            span class="pill status-working" { "Working" }
                        }
                        @if !queue.items.is_empty() {
                            span class="pill" { "Queued " (queue.items.len()) }
                        }
                    }
                }
                p class="text-destructive" data-show="$chatError" data-text="$chatError" {}
                div class="chat-input-wrapper" {
                    input
                        type="text"
                        name="content"
                        placeholder="Message Shepherd..."
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
    work_tree: &[WorkItemTree],
    workers: &[Worker],
    efforts: &[ShepherdEffort],
    focused_effort: Option<&ShepherdEffort>,
    history: &[ShepherdChatMessage],
    queue: &ShepherdQueueState,
    notifications: &UnreadNotificationsResponse,
) -> Markup {
    let default_machinery_tab = if !efforts.is_empty() || !workers.is_empty() {
        "agents"
    } else {
        "work"
    };
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
            main class="app-shell" data-signals:machinery-open="false" data-signals:machinery-tab=(format!("'{}'", default_machinery_tab)) data-signals:project-picker-open="false" {
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

                        // ── Surface Toolbar ──
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
                            button type="button" class="btn btn-sm btn-ghost toolbar-toggle" data-on:click="$machineryOpen = !$machineryOpen" {
                                "Machinery"
                            }
                        }

                        // ── Focus Stage ──
                        section id="focus-panel" class="focus-stage" {
                            // Tab bar: Project + active efforts
                            @if !efforts.is_empty() {
                                nav class="focus-tabs" {
                                    form action=(format!("/app/projects/{}/efforts/unfocus", project.id)) method="post" {
                                        button type="submit" class=(if focused_effort.is_none() { "focus-tab active" } else { "focus-tab" }) {
                                            (icon("folder"))
                                            "Project"
                                        }
                                    }
                                    @for effort in efforts.iter().take(MAX_VISIBLE_EFFORTS) {
                                        @let is_active = focused_effort.map(|e| e.id.as_str()) == Some(effort.id.as_str());
                                        form action=(format!("/app/projects/{}/efforts/{}/focus", project.id, effort.id)) method="post" {
                                            button type="submit" class=(if is_active { "focus-tab active" } else { "focus-tab" }) {
                                                span { (&effort.title) }
                                                @if effort.status != "active" {
                                                    span class=(format!("focus-tab-status status-{}", effort.status)) { "·" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Content area
                            div class="focus-content" {
                                @if let Some(effort) = focused_effort {
                                    // Show effort focus HTML or empty state
                                    @if let Some(ref html) = effort.focus_html {
                                        iframe
                                            title={ "Effort: " (&effort.title) }
                                            srcdoc=(html)
                                            class="focus-frame" {}
                                    } @else {
                                        div class="empty-focus-state" {
                                            svg class="empty-glyph" viewBox="0 0 40 40" fill="none" stroke="currentColor" stroke-width="0.75" {
                                                rect x="4" y="4" width="32" height="32" {}
                                                line x1="4" y1="20" x2="36" y2="20" {}
                                                line x1="20" y1="4" x2="20" y2="36" {}
                                                rect x="12" y="12" width="16" height="16" opacity="0.35" {}
                                            }
                                            p class="eyebrow" { (&effort.title) }
                                            p class="muted" { "Shepherd will populate this view as the effort progresses." }
                                        }
                                    }
                                } @else if has_focus {
                                    // Project-level focus
                                    iframe
                                        title={ "Project focus for " (&project.name) }
                                        src={ "/app/projects/" (project.id) "/focus" }
                                        class="focus-frame" {}
                                } @else {
                                    // No focus yet — efforts (including sync) show in the tab bar
                                    div class="empty-focus-state" {
                                        svg class="empty-glyph" viewBox="0 0 40 40" fill="none" stroke="currentColor" stroke-width="0.75" {
                                            rect x="4" y="4" width="32" height="32" {}
                                            line x1="4" y1="20" x2="36" y2="20" {}
                                            line x1="20" y1="4" x2="20" y2="36" {}
                                            rect x="12" y="12" width="16" height="16" opacity="0.35" {}
                                        }
                                        p class="eyebrow" { "Project overview" }
                                        p class="muted" { "Select an effort above or send a message to get started." }
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
                                        button type="submit" class="btn btn-sm btn-ghost" {
                                            (icon("copy-plus"))
                                            "Fork"
                                        }
                                    }
                                    @if surface.routes.len() > 1 {
                                        form action={ "/app/projects/" (project.id) "/routes/" (route.id) "/archive" } method="post" {
                                            button type="submit" class="btn btn-sm btn-ghost btn-danger" {
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
                                        data-class:aria-selected="$machineryTab === 'agents'"
                                        data-on:click="$machineryTab = 'agents'" {
                                        "Agents"
                                    }
                                }
                            }
                            div class="machinery-body" {
                                section id="work-panel" class="machinery-panel" data-show="$machineryTab === 'work'" {
                                    (render_work_tree_nodes(work_tree))
                                }
                                section id="agents-panel" class="machinery-panel" data-show="$machineryTab === 'agents'" {
                                    (render_agent_cards(efforts, focused_effort, workers))
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
                    div class="panel-header" {
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
                        a href=(format!("/app/projects/{}", project.id)) class="btn btn-ghost" {
                            (icon("arrow-left"))
                            "Back"
                        }
                    }

                    // ── Events ──
                    article class="card" {
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
