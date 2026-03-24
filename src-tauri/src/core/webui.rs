use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::core::api_types::{Worker, WorkerEventResponse};
use crate::core::project::{Project, ProjectSurfaceSnapshot};
use crate::core::route::Route;
use crate::core::worktree::{WorkItemTree, SYNC_PROJECT_TASK_MARKER, SYNC_PROJECT_TASK_TITLE};
use crate::core::ShepherdChatMessage;
use crate::gui::commands::shepherd::commands::ShepherdQueueState;
use crate::gui::commands::types::UnreadNotificationsResponse;

const DATASTAR_BUNDLE: &str = "/static/datastar.js";

fn page_head(title: &str, description: &str) -> Markup {
    html! {
        meta charset="utf-8";
        meta name="viewport" content="width=device-width, initial-scale=1";
        title { (title) " · Hirsel" }
        meta name="description" content=(description);
        meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; frame-src 'self'; connect-src 'self';";
        link rel="stylesheet" href="/static/webui.css";
        script type="module" src=(DATASTAR_BUNDLE) {}
    }
}

fn app_document(title: &str, description: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                (page_head(title, description))
            }
            body {
                (body)
            }
        }
    }
}

fn format_time(timestamp: &str) -> &str {
    timestamp
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
                                span class=(format!("status-pill status-{}", node.status)) { (&node.status) }
                                @if let Some(profile) = node.capability_profile {
                                    span class="capability-pill" { (profile.as_str()) }
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

pub fn render_worker_cards(workers: &[Worker]) -> Markup {
    html! {
        @if workers.is_empty() {
            div class="empty-card" {
                p { "No workers are active on this route." }
            }
        } @else {
            div class="worker-grid" {
                @for worker in workers {
                    article class="worker-card" {
                        div class="worker-heading" {
                            h3 { (&worker.name) }
                                    span class=(format!("status-pill status-{}", format!("{:?}", worker.status).to_lowercase())) {
                                (format!("{:?}", worker.status).to_lowercase())
                            }
                        }
                        @if let Some(task) = &worker.current_task {
                            p class="muted" { (task) }
                        }
                        @if let Some(profile) = &worker.capability_profile {
                            p class="muted small" { "Capability: " (profile.as_str()) }
                        }
                    }
                }
            }
        }
    }
}

pub fn render_chat_panel(
    project_id: i64,
    history: &[ShepherdChatMessage],
    queue: &ShepherdQueueState,
) -> Markup {
    html! {
        section id="chat-panel" class="chat-panel" {
            div class="panel-header" {
                h2 { "Shepherd" }
                @if queue.has_active_turn {
                    span class="muted small" { "Working" }
                }
            }
            div id="chat-thread" class="chat-thread" {
                @if history.is_empty() && queue.items.is_empty() {
                    article class="message message-assistant" {
                        p { "Hello. I can help steer the project surface, routes, work items, and workers." }
                    }
                } @else {
                    @for message in history {
                        article class=(format!("message message-{}", message.role)) {
                            div class="message-meta" {
                                span { (&message.role) }
                                span { (format_time(&message.timestamp)) }
                            }
                            pre class="message-body" { (render_chat_text(&message.chunks_json)) }
                        }
                    }
                    @for item in &queue.items {
                        article class="message message-user pending" {
                            div class="message-meta" {
                                span { "user" }
                                span { (format_time(&item.created_at)) }
                            }
                            pre class="message-body" { (render_chat_text(&item.chunks_json)) }
                            p class="muted small" {
                                @if item.status == "working" {
                                    "Processing on server"
                                } @else if item.status == "failed" {
                                    "Failed on server"
                                } @else {
                                    "Queued on server"
                                }
                            }
                            @if let Some(error) = &item.error {
                                p class="error-text" { (error) }
                            }
                        }
                    }
                }
            }
            form
                class="chat-composer"
                data-signals:chat-draft="''"
                data-signals:chat-sending="false"
                data-indicator:chat-sending
                data-on:submit__prevent=(format!("@post('/app/projects/{}/chat/send')", project_id)) {
                textarea
                    name="content"
                    rows="4"
                    placeholder="Message Shepherd..."
                    data-bind:chat-draft {}
                div class="composer-row" {
                    button
                        type="submit"
                        class="primary-btn"
                        data-attr:disabled="$chatSending || !$chatDraft.trim()" {
                        "Send"
                    }
                }
            }
        }
    }
}

pub fn render_connect_page(error: Option<&str>, return_to: Option<&str>) -> Markup {
    app_document(
        "Connect",
        "Connect to a Hirsel backend",
        html! {
            main class="connect-page" {
                section class="connect-card" {
                    h1 { "Connect to Hirsel" }
                    p class="muted" { "Enter the backend API key to open this Hirsel server." }
                    @if let Some(error) = error {
                        p class="error-text" { (error) }
                    }
                    form action="/connect/session" method="post" class="stack" {
                        input type="hidden" name="return_to" value=(return_to.unwrap_or("/app"));
                        label class="stack" {
                            span { "API key" }
                            input type="password" name="api_key" autocomplete="current-password" required;
                        }
                        button type="submit" class="primary-btn" { "Open Hirsel" }
                    }
                }
            }
        },
    )
}

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
                            a href=(format!("/app/projects/{}", project.id)) class="project-link" { (&project.name) }
                        }
                    }
                    a href="/app/settings" class="ghost-btn full" { "Backend settings" }
                }
                section class="main-panel" {
                    article class="form-card" {
                        h1 { "Create project" }
                        form action="/app/projects" method="post" class="stack" {
                            label class="stack" {
                                span { "Project name" }
                                input type="text" name="name" required;
                            }
                            label class="stack" {
                                span { "Repository URL" }
                                input type="url" name="repo_url" placeholder="https://github.com/owner/repo" required;
                            }
                            label class="stack" {
                                span { "Branch" }
                                input type="text" name="branch" value="main";
                            }
                            button type="submit" class="primary-btn" { "Create project" }
                        }
                    }
                }
            }
        },
    )
}

pub fn render_project_page(
    projects: &[Project],
    project: &Project,
    route: &Route,
    surface: &ProjectSurfaceSnapshot,
    work_tree: &[WorkItemTree],
    workers: &[Worker],
    history: &[ShepherdChatMessage],
    queue: &ShepherdQueueState,
    notifications: &UnreadNotificationsResponse,
) -> Markup {
    let sync_state = sync_task(work_tree)
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
            main class="shell" data-signals:machinery-open="true" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}
                aside class="sidebar" id="project-sidebar" {
                    div class="brand" { "HIRSEL" }
                    nav class="project-nav" {
                        @for item in projects {
                            a
                                href=(format!("/app/projects/{}", item.id))
                                class=(if item.id == project.id { "project-link active" } else { "project-link" }) {
                                (&item.name)
                            }
                        }
                    }
                    div class="sidebar-actions" {
                        a href="/app" class="ghost-btn full" { "New project" }
                        a href="/app/settings" class="ghost-btn full" { "Backend settings" }
                        a href=(format!("/app/projects/{}/settings", project.id)) class="ghost-btn full" { "Project settings" }
                    }
                }
                section class="workspace" {
                    header id="project-header" class="topbar" {
                        div {
                            h1 { (&project.name) }
                            p class="muted small" { "Route " (&route.name) }
                        }
                        div class="topbar-actions" {
                            span class="notification-pill" { (notifications.notifications.len()) " unread" }
                            button type="button" class="ghost-btn" data-on:click="$machineryOpen = !$machineryOpen" {
                                "Machinery"
                            }
                        }
                    }
                    div class="workspace-grid" {
                        section id="focus-panel" class="focus-panel" {
                            @if has_focus {
                                iframe
                                    title={ "Project focus for " (&project.name) }
                                    src={ "/app/projects/" (project.id) "/focus" }
                                    class="focus-frame" {}
                            } @else {
                                div class="empty-focus" {
                                    p class="eyebrow" { "Project focus view" }
                                    h2 { "Awaiting project focus" }
                                    @if sync_state == "working" {
                                        p class="muted" { "Hirsel is surveying the project in the background." }
                                    } @else if sync_state == "failed" {
                                        p class="muted" { "Project sync failed. Retry to build the first project picture." }
                                    } @else {
                                        p class="muted" { "Generate the first project picture when you are ready." }
                                    }
                                    form action={ "/app/projects/" (project.id) "/sync" } method="post" {
                                        button type="submit" class="primary-btn" {
                                            @if sync_state == "working" { "Syncing" } @else if sync_state == "failed" { "Retry sync" } @else { "Sync" }
                                        }
                                    }
                                }
                            }
                        }
                        (render_chat_panel(project.id, history, queue))
                    }
                    section class="machinery" data-show="$machineryOpen" {
                        div class="panel-header" {
                            div class="route-toolbar" {
                                @for item in &surface.routes {
                                    form action=(format!("/app/projects/{}/routes/{}/select", project.id, item.route_id)) method="post" {
                                        button
                                            type="submit"
                                            class=(if item.route_id == route.id { "route-btn active" } else { "route-btn" }) {
                                            (&item.name) " · " (&item.status)
                                        }
                                    }
                                }
                            }
                            div class="route-actions" {
                                form action=(format!("/app/projects/{}/routes", project.id)) method="post" class="inline-form" {
                                    input type="text" name="name" placeholder="New route name" required;
                                    button type="submit" class="ghost-btn" { "Fork route" }
                                }
                                @if surface.routes.len() > 1 {
                                    form action={ "/app/projects/" (project.id) "/routes/" (route.id) "/archive" } method="post" {
                                        button type="submit" class="ghost-btn danger" { "Archive route" }
                                    }
                                }
                            }
                        }
                        div class="machinery-grid" {
                            section id="work-panel" class="panel-card" {
                                div class="panel-header" {
                                    h2 { "Work" }
                                }
                                (render_work_tree_nodes(work_tree))
                            }
                            section id="workers-panel" class="panel-card" {
                                div class="panel-header" {
                                    h2 { "Workers" }
                                }
                                (render_worker_cards(workers))
                            }
                        }
                    }
                }
            }
        },
    )
}

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
                    article class="form-card" {
                        div class="panel-header" {
                            h1 { "Backend settings" }
                            a href="/app" class="ghost-btn" { "Back" }
                        }
                        form action="/app/settings/llm" method="post" class="stack" {
                            label class="stack" {
                                span { "Provider" }
                                select name="provider" {
                                    option value="codex" selected[provider == "codex"] { "Codex" }
                                    option value="openrouter" selected[provider == "openrouter"] { "OpenRouter" }
                                }
                            }
                            label class="stack" {
                                span { "OpenRouter base URL" }
                                input type="text" name="openrouter_base_url" value=(openrouter_base_url.unwrap_or(""));
                            }
                            button type="submit" class="primary-btn" { "Save provider" }
                        }
                    }
                    article class="form-card" {
                        h2 { "Credentials" }
                        form action="/app/settings/openrouter" method="post" class="stack" {
                            p class="muted small" { "OpenRouter API key" }
                            @if let Some(masked) = openrouter_masked {
                                p class="muted" { "Current: " (masked) }
                            }
                            input type="password" name="api_key" placeholder="sk-or-..." ;
                            button type="submit" class="primary-btn" { "Save OpenRouter key" }
                        }
                        form action="/app/settings/tavily" method="post" class="stack" {
                            p class="muted small" { "Tavily API key" }
                            @if let Some(masked) = tavily_masked {
                                p class="muted" { "Current: " (masked) }
                            }
                            input type="password" name="api_key" placeholder="tvly-..." ;
                            button type="submit" class="ghost-btn" { "Save Tavily key" }
                        }
                    }
                    article class="form-card" id="codex-status" {
                        h2 { "Codex" }
                        @if codex_connected {
                            p { "Codex is connected." }
                        } @else if let Some((device_auth_id, user_code, verify_url)) = codex_state {
                            div data-init=(format!("@get('/app/settings/codex/stream?device_auth_id={}&user_code={}', {{openWhenHidden: true}})", device_auth_id, user_code)) {}
                            p class="muted" { "Open the verification page, enter the code, and keep this page open." }
                            p class="code-block" { (user_code) }
                            p { a href=(verify_url) target="_blank" rel="noreferrer" { "Open Codex verification" } }
                            p class="muted small" { "Waiting for approval…" }
                        } @else {
                            form action="/app/settings/codex/start" method="post" {
                                button type="submit" class="primary-btn" { "Connect Codex" }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub fn render_project_settings_page(project: &Project, route: &Route) -> Markup {
    app_document(
        "Project settings",
        "Configure project and route defaults",
        html! {
            main class="shell shell-single" {
                section class="main-panel main-panel-narrow" {
                    article class="form-card" {
                        div class="panel-header" {
                            h1 { "Project settings" }
                            a href=(format!("/app/projects/{}", project.id)) class="ghost-btn" { "Back" }
                        }
                        form action={ "/app/projects/" (project.id) "/settings/project" } method="post" class="stack" {
                            label class="stack" {
                                span { "Project name" }
                                input type="text" name="name" value=(&project.name);
                            }
                            label class="stack" {
                                span { "Description" }
                                textarea name="description" rows="5" { (project.description.as_deref().unwrap_or("")) }
                            }
                            button type="submit" class="primary-btn" { "Save project" }
                        }
                    }
                    article class="form-card" {
                        h2 { "Selected route" }
                        form action={ "/app/projects/" (project.id) "/settings/route" } method="post" class="stack" {
                            input type="hidden" name="route_id" value=(route.id);
                            p class="muted" { "Current route: " (&route.name) }
                            label class="stack" {
                                span { "Time limit (minutes)" }
                                input type="number" min="0" name="time_limit_minutes" value=(route.time_limit_minutes.unwrap_or_default());
                            }
                            label class="inline-check" {
                                input type="checkbox" name="human_in_the_loop" checked[route.human_in_the_loop];
                                span { "Require human in the loop" }
                            }
                            label class="stack" {
                                span { "Target branch" }
                                input type="text" name="target_branch" value=(route.target_branch.as_deref().unwrap_or(""));
                            }
                            button type="submit" class="primary-btn" { "Save route" }
                        }
                    }
                }
            }
        },
    )
}

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
                    article class="form-card" {
                        div class="panel-header" {
                            h1 { (&worker.name) }
                            a href=(format!("/app/projects/{}", project.id)) class="ghost-btn" { "Back" }
                        }
                        p class="muted" { "Route " (&route.name) " · " (format!("{:?}", worker.status).to_lowercase()) }
                        section id="worker-events" class="worker-events" {
                            @for event in events {
                                article class="event-row" {
                                    div class="message-meta" {
                                        span { (&event.event_type) }
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
        },
    )
}

pub fn render_focus_document(html_doc: &str) -> Markup {
    PreEscaped(html_doc.to_string())
}
