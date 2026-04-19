//! Librarian — knowledge graph agent backed by SurrealDB.
//!
//! The Librarian runs as one-shot background jobs. Each user-visible chat
//! turn and each periodic lint pass enqueues a `librarian_job` row; a worker
//! loop claims rows and runs a disposable Lash runtime that has exactly the
//! graph-patch tools it needs. There is no long-lived librarian conversation:
//! every job starts fresh, re-derives context via `search_graph`, and exits.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::tools::{Glob as LashGlob, Grep as LashGrep, Ls as LashLs, ReadFilePluginFactory};
use lash::{
    default_execution_mode, BuiltinToolResultProjectionPluginFactory, EventSink,
    FsInstructionSource, HostProfile, InputItem, InstructionSource, LashRuntime, PluginHost,
    PluginSpec, PromptContribution, PromptOverrideMode, PromptSectionName, PromptSectionOverride,
    RuntimeHostConfig, RuntimeServices, SessionEvent, SessionPolicy, SessionStateEnvelope,
    ToolDefinition, ToolParam, ToolProvider, ToolResult, TurnInput,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use surrealdb::types::{RecordId, SurrealValue};
use tokio_util::sync::CancellationToken;

use crate::backend::db::{global_db, librarian_db};
use crate::backend::documents::{self, DocumentValidationError};
use crate::backend::live_updates::{self, LiveUpdateKind};
use crate::backend::llm_provider::{self, RuntimeModelRole};
use crate::backend::shepherd_runtime::{ShepherdMessageChunk, ShepherdScope};
use crate::backend::text_patch::{apply_text_patch, TEXT_PATCH_INSTRUCTIONS};
use crate::backend::ProjectStore;

const GRAPH_MUTATION_KEYWORDS: &[&str] =
    &["CREATE", "UPSERT", "UPDATE", "DELETE", "RELATE", "INSERT"];

const CANVAS_TAGS_REQUIRING_NODE_ATTR: &[&str] = &[
    "hirsel-node-ref",
    "hirsel-node-field",
    "hirsel-node-list",
    "hirsel-doc-target",
    "hirsel-doc-link",
    "hirsel-doc-embed",
];

const WRITABLE_TEXT_FIELDS: &[&str] = &["content"];
const READABLE_TEXT_FIELDS: &[&str] = &["content", "label", "summary"];
const MAX_SEARCH_LIMIT: usize = 25;
const MAX_LINES_PER_READ: usize = 400;

const LINT_PROMPT: &str = "\
Review the knowledge graph for health:
- Orphan nodes: nodes with no edges and no tags, not referenced by document:index
- Stale nodes: read_by_search_context is recent but updated_at is old (high-value, going stale)
- Unused nodes: read_by_search_context is null and updated_at is old (nobody needs this)
- Missing nodes: concepts mentioned in existing node content that lack their own node
- Weak content: nodes with labels but trivial/empty content
- Index drift: document:index that doesn't reflect the current set of nodes

Use `search_graph` to discover nodes, `read_node` / `read_node_property` to inspect them,
and `apply_graph_patch` to fix structural issues. Report a brief summary of what changed.
If nothing needs changing, say so and stop.";

// ── Public helpers callable from the ingress side ──

pub async fn queue_background_sync(
    project_id: i64,
    source_scope: &ShepherdScope,
    user_chunks: &[ShepherdMessageChunk],
    assistant_chunks: &[ShepherdMessageChunk],
) -> Result<(), String> {
    let source_label = sync_source_label(source_scope);
    let user_message = format_sync_chunks(user_chunks);
    let assistant_message = format_sync_chunks(assistant_chunks);
    let prompt = format!(
        "You are the project Librarian. The user just completed a turn in the {source_label} scope.\n\n\
         Extract any durable, graph-worthy knowledge from it — new components, decisions, facts, \
         conventions, goals, or relationships — and update the knowledge graph with `apply_graph_patch`. \
         Prefer updating existing nodes (use `search_graph` first). Skip ephemeral chat. If nothing is \
         graph-worthy, reply briefly and stop.\n\n\
         ## User message\n\n{user_message}\n\n## Assistant reply\n\n{assistant_message}\n"
    );
    LibrarianJobStore::open()
        .await?
        .enqueue(project_id, LibrarianJobKind::Sync, &prompt)
        .await
        .map(|_| ())
}

pub async fn enqueue_librarian_lint(project_id: i64) -> Result<(), String> {
    LibrarianJobStore::open()
        .await?
        .enqueue(project_id, LibrarianJobKind::Lint, LINT_PROMPT)
        .await
        .map(|_| ())
}

fn sync_source_label(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::Shepherd { .. } => "Shepherd".to_string(),
        ShepherdScope::Thread {
            title, thread_id, ..
        } => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                format!("Thread: {thread_id}")
            } else {
                format!("Thread: {trimmed}")
            }
        }
        ShepherdScope::General => "General".to_string(),
    }
}

fn format_sync_chunks(chunks: &[ShepherdMessageChunk]) -> String {
    let mut lines = Vec::new();
    let mut image_count = 0usize;

    for chunk in chunks {
        match chunk {
            ShepherdMessageChunk::Text { content } => {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }
            ShepherdMessageChunk::Notice { title, content, .. } => {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let prefix = title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("[{value}] "))
                    .unwrap_or_default();
                lines.push(format!("{prefix}{trimmed}"));
            }
            ShepherdMessageChunk::Image { .. } => image_count += 1,
            ShepherdMessageChunk::Tool { .. }
            | ShepherdMessageChunk::Skill { .. }
            | ShepherdMessageChunk::FileRef { .. }
            | ShepherdMessageChunk::Thinking { .. } => {}
        }
    }

    if image_count > 0 {
        lines.push(format!(
            "[{} image attachment{}]",
            image_count,
            if image_count == 1 { "" } else { "s" }
        ));
    }

    let joined = lines.join("\n\n");
    if joined.trim().is_empty() {
        "[No user-visible content]".to_string()
    } else {
        joined
    }
}

// ── Job store ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarianJobKind {
    Sync,
    Lint,
}

impl LibrarianJobKind {
    fn as_str(self) -> &'static str {
        match self {
            LibrarianJobKind::Sync => "sync",
            LibrarianJobKind::Lint => "lint",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "sync" => Some(LibrarianJobKind::Sync),
            "lint" => Some(LibrarianJobKind::Lint),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, SurrealValue)]
struct LibrarianJobRow {
    id: RecordId,
    project_id: i64,
    kind: String,
    prompt: String,
    #[serde(default)]
    #[allow(dead_code)]
    status: String,
    #[serde(default)]
    #[allow(dead_code)]
    last_error: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[allow(dead_code)]
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct LibrarianJob {
    id: RecordId,
    project_id: i64,
    kind: LibrarianJobKind,
    prompt: String,
}

impl TryFrom<LibrarianJobRow> for LibrarianJob {
    type Error = String;
    fn try_from(row: LibrarianJobRow) -> Result<Self, Self::Error> {
        let kind = LibrarianJobKind::parse(&row.kind)
            .ok_or_else(|| format!("unknown librarian job kind: {}", row.kind))?;
        Ok(LibrarianJob {
            id: row.id,
            project_id: row.project_id,
            kind,
            prompt: row.prompt,
        })
    }
}

struct LibrarianJobStore;

impl LibrarianJobStore {
    async fn open() -> Result<Self, String> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn enqueue(
        &self,
        project_id: i64,
        kind: LibrarianJobKind,
        prompt: &str,
    ) -> Result<RecordId, String> {
        let db = global_db().await;
        let mut response = db
            .query(
                "CREATE librarian_job CONTENT { \
                 project_id: $project_id, kind: $kind, prompt: $prompt, status: 'queued' \
                 } RETURN id;",
            )
            .bind(("project_id", project_id))
            .bind(("kind", kind.as_str().to_string()))
            .bind(("prompt", prompt.to_string()))
            .await
            .map_err(|e| format!("failed to enqueue librarian job: {e}"))?;
        let rows: Vec<LibrarianJobRow> = response.take(0).unwrap_or_default();
        rows.into_iter()
            .next()
            .map(|row| row.id)
            .ok_or_else(|| "librarian job enqueue returned no row".to_string())
    }

    async fn claim_next(&self) -> Result<Option<LibrarianJob>, String> {
        let db = global_db().await;
        let mut response = db
            .query(
                "LET $jobs = (SELECT id, created_at FROM librarian_job WHERE status = 'queued' ORDER BY created_at ASC LIMIT 1); \
                 UPDATE $jobs SET status = 'running'; \
                 SELECT * FROM $jobs;",
            )
            .await
            .map_err(|e| format!("failed to claim librarian job: {e}"))?;
        let rows: Vec<LibrarianJobRow> = response.take(2).unwrap_or_default();
        rows.into_iter()
            .next()
            .map(LibrarianJob::try_from)
            .transpose()
    }

    async fn mark_finished(&self, id: &RecordId) -> Result<(), String> {
        let db = global_db().await;
        db.query("UPDATE $id SET status = 'completed', last_error = NONE")
            .bind(("id", id.clone()))
            .await
            .map_err(|e| format!("failed to mark librarian job completed: {e}"))?;
        Ok(())
    }

    async fn mark_failed(&self, id: &RecordId, error: &str) -> Result<(), String> {
        let db = global_db().await;
        db.query("UPDATE $id SET status = 'failed', last_error = $error")
            .bind(("id", id.clone()))
            .bind(("error", error.to_string()))
            .await
            .map_err(|e| format!("failed to mark librarian job failed: {e}"))?;
        Ok(())
    }

    /// On startup, any `running` rows are stragglers from a previous process —
    /// re-queue them so the new worker picks them up.
    async fn requeue_running(&self) -> Result<(), String> {
        let db = global_db().await;
        db.query("UPDATE librarian_job SET status = 'queued' WHERE status = 'running'")
            .await
            .map_err(|e| format!("failed to requeue stale librarian jobs: {e}"))?;
        Ok(())
    }
}

// ── Worker loop ──

/// Spawn the background librarian worker. Polls `librarian_job` and runs one
/// disposable agent session per row.
pub fn spawn_worker() {
    tokio::spawn(async move {
        let store = match LibrarianJobStore::open().await {
            Ok(store) => store,
            Err(error) => {
                tracing::error!(%error, "librarian worker failed to open job store");
                return;
            }
        };
        if let Err(error) = store.requeue_running().await {
            tracing::warn!(%error, "failed to requeue stale librarian jobs");
        }

        loop {
            match store.claim_next().await {
                Ok(Some(job)) => {
                    let job_id = job.id.clone();
                    let project_id = job.project_id;
                    let kind = job.kind.as_str();
                    tracing::info!(project_id, kind, "librarian job started");
                    match run_librarian_job(job).await {
                        Ok(_) => {
                            if let Err(error) = store.mark_finished(&job_id).await {
                                tracing::warn!(%error, project_id, kind, "failed to mark job finished");
                            } else {
                                tracing::info!(project_id, kind, "librarian job completed");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, project_id, kind, "librarian job failed");
                            let _ = store.mark_failed(&job_id, &error).await;
                        }
                    }
                }
                Ok(None) => {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(error) => {
                    tracing::error!(%error, "librarian job claim failed");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}

/// Periodic lint trigger. Runs every 30 minutes after a 5-minute warm-up.
pub fn spawn_periodic_lint() {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        loop {
            if let Ok(store) = ProjectStore::open().await {
                if let Ok(projects) = store.list_projects().await {
                    for project in projects {
                        let _ = enqueue_librarian_lint(project.id).await;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1800)).await;
        }
    });
}

/// Periodic comment-thread summarisation sweep. Finds nodes whose
/// unresolved comment count exceeds the configured threshold, compiles a
/// summary of those comments, writes the summary as a new comment, and
/// marks the originals as resolved. Interval is setting-driven.
pub fn spawn_comment_summariser() {
    tokio::spawn(async move {
        use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};
        tokio::time::sleep(Duration::from_secs(180)).await;
        loop {
            let interval: u64 = RuntimeSettings::get_or(
                keys::GRAPH_COMMENT_SUMMARY_INTERVAL_SEC,
                Defaults::GRAPH_COMMENT_SUMMARY_INTERVAL_SEC,
            )
            .await;
            if let Err(error) = run_comment_summariser_sweep().await {
                tracing::warn!(%error, "comment summariser sweep failed");
            }
            tokio::time::sleep(Duration::from_secs(interval.max(60))).await;
        }
    });
}

async fn run_comment_summariser_sweep() -> Result<(), String> {
    use crate::backend::kg_comment::CommentStore;
    use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

    let threshold: usize = RuntimeSettings::get_or(
        keys::GRAPH_COMMENT_SUMMARY_THRESHOLD,
        Defaults::GRAPH_COMMENT_SUMMARY_THRESHOLD,
    )
    .await;

    let store = CommentStore::open()
        .await
        .map_err(|e| e.to_string())?;
    let counts = store
        .unresolved_counts_by_node()
        .await
        .map_err(|e| e.to_string())?;

    for (project_id, node_kind, node_id, count) in counts {
        if count < threshold {
            continue;
        }
        let comments = store
            .list_for_node(project_id, &node_kind, &node_id, count, true)
            .await
            .map_err(|e| e.to_string())?;
        if comments.is_empty() {
            continue;
        }
        let summary_body = compose_comment_summary(&comments);
        let original_ids: Vec<String> = comments.iter().map(|c| c.id.clone()).collect();
        if let Err(error) = store
            .summarise_and_resolve(
                project_id,
                &node_kind,
                &node_id,
                &summary_body,
                "librarian",
                &original_ids,
            )
            .await
        {
            tracing::warn!(%error, project_id, node_kind, node_id, "comment summarise failed");
            continue;
        }
        tracing::info!(
            project_id,
            node_kind,
            node_id,
            collapsed = original_ids.len(),
            "summarised comment thread"
        );
    }

    Ok(())
}

fn compose_comment_summary(comments: &[crate::backend::kg_comment::Comment]) -> String {
    // v1: deterministic rollup — one bullet per author, joined with newlines.
    // LLM-backed summaries can swap in later by delegating to a sub-runtime.
    let mut by_author: std::collections::BTreeMap<String, Vec<&str>> =
        std::collections::BTreeMap::new();
    for c in comments {
        let first_line = c.body.lines().next().unwrap_or(&c.body);
        by_author.entry(c.author.clone()).or_default().push(first_line);
    }
    let mut out = format!(
        "Summary of {} unresolved comments (auto-rolled up by librarian):\n",
        comments.len()
    );
    for (author, lines) in by_author {
        out.push_str(&format!("- @{author}: "));
        out.push_str(&lines.join(" ｜ "));
        out.push('\n');
    }
    out
}

/// Orphan-workspace cleanup sweep. Finds thread workspace copies whose
/// parent thread no longer exists (or was archived) and whose last
/// activity exceeds the configured TTL; discards them unless cleanup is
/// disabled. Runs every 4 hours after a short warm-up.
pub fn spawn_orphan_workspace_cleanup() {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(120)).await;
        loop {
            if let Err(error) = run_orphan_workspace_sweep().await {
                tracing::warn!(%error, "orphan workspace sweep failed");
            }
            tokio::time::sleep(Duration::from_secs(4 * 60 * 60)).await;
        }
    });
}

async fn run_orphan_workspace_sweep() -> Result<(), String> {
    use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};
    use crate::backend::{workspace_copy, ShepherdThreadStore};
    use std::path::PathBuf;

    let mode: String = RuntimeSettings::get_or(
        keys::WORKSPACE_ORPHAN_CLEANUP,
        Defaults::WORKSPACE_ORPHAN_CLEANUP.to_string(),
    )
    .await;
    if mode == "off" {
        return Ok(());
    }
    let ttl_days: u64 = RuntimeSettings::get_or(
        keys::WORKSPACE_ORPHAN_TTL_DAYS,
        Defaults::WORKSPACE_ORPHAN_TTL_DAYS,
    )
    .await;

    let db = crate::backend::db::global_db().await;
    let mut response = db
        .query(
            "SELECT thread_id, parent_id, workspace_path, archived_at, last_activity_at \
             FROM shepherd_thread WHERE workspace_path != NONE",
        )
        .await
        .map_err(|e| format!("orphan sweep query failed: {e}"))?;
    let rows: Vec<serde_json::Value> = response.take(0).unwrap_or_default();
    if rows.is_empty() {
        return Ok(());
    }

    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| e.to_string())?;

    let now = chrono::Utc::now();
    let ttl = chrono::Duration::days(ttl_days as i64);

    for row in rows {
        let Some(thread_id) = row.get("thread_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let workspace_path = row
            .get("workspace_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);
        let parent_id = row.get("parent_id").and_then(|v| v.as_str());
        let archived_at = row.get("archived_at").and_then(|v| v.as_str());
        let last_activity_at = row.get("last_activity_at").and_then(|v| v.as_str());

        let parent_missing = if let Some(pid) = parent_id {
            store.get_thread(pid).await.is_err()
        } else {
            // No parent — never orphaned by that criterion alone.
            false
        };
        let stale = last_activity_at
            .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|ts| now.signed_duration_since(ts) > ttl)
            .unwrap_or(false);

        let is_candidate = (parent_missing || archived_at.is_some()) && stale;
        if !is_candidate {
            continue;
        }

        match mode.as_str() {
            "auto" => {
                if let Some(path) = workspace_path.as_ref() {
                    let fake = workspace_copy::WorkspaceCopy {
                        thread_id: thread_id.to_string(),
                        canonical: PathBuf::new(),
                        copy_dir: path.clone(),
                        base_commit: None,
                    };
                    if let Err(error) = workspace_copy::discard(&fake).await {
                        tracing::warn!(%error, thread_id, "orphan discard failed");
                        continue;
                    }
                }
                let _ = store
                    .set_thread_workspace_path(thread_id, None)
                    .await;
                let _ = store
                    .set_thread_merge_status(thread_id, "discarded")
                    .await;
                tracing::info!(
                    thread_id,
                    ?workspace_path,
                    "auto-discarded orphan thread workspace copy"
                );
            }
            _ => {
                // "prompt" mode: just flag the merge_status so the UI
                // surfaces the orphan for user action. Actual discard
                // happens via explicit discard_thread.
                let _ = store
                    .set_thread_merge_status(thread_id, "orphaned")
                    .await;
                tracing::info!(
                    thread_id,
                    ?workspace_path,
                    "flagged orphan thread workspace copy for review"
                );
            }
        }
    }

    Ok(())
}

// ── Session runner ──

async fn run_librarian_job(job: LibrarianJob) -> Result<String, String> {
    let LibrarianJob {
        project_id,
        prompt,
        kind,
        ..
    } = job;
    let workspace_root = resolve_project_workspace(project_id).await;

    let settings = crate::backend::AppSettingsStore::open()
        .await
        .map_err(|e| format!("failed to open app settings: {e}"))?
        .load_llm_settings()
        .await
        .map_err(|e| format!("failed to load llm settings: {e}"))?;
    let provider = llm_provider::resolve_provider(&settings).await?;
    let (model, model_variant) =
        llm_provider::resolve_model_for_role(&settings, &provider, RuntimeModelRole::Librarian);
    let execution_mode = default_execution_mode();
    let session_id = format!("librarian-{}-{}", kind.as_str(), project_id);

    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::backend::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(session_id.clone()),
        execution_mode,
        ..Default::default()
    };

    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: workspace_root.clone(),
        prompt_overrides: vec![PromptSectionOverride {
            section: PromptSectionName::Guidance,
            block: None,
            mode: PromptOverrideMode::Append,
            content: librarian_system_prompt(project_id, workspace_root.as_deref()),
        }],
        ..RuntimeHostConfig::default()
    };

    let services = build_runtime_services(project_id, workspace_root.as_deref(), &session_id)?;

    let state = SessionStateEnvelope {
        session_id: session_id.clone(),
        policy: session_policy.clone(),
        ..SessionStateEnvelope::default()
    };

    let mut runtime = LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create librarian runtime: {e}"))?;

    let sink = NoopEventSink;
    let cancel = CancellationToken::new();
    let turn_input = TurnInput {
        items: vec![InputItem::Text { text: prompt }],
        image_blobs: Default::default(),
        mode: None,
        user_input: None,
    };

    let turn = tokio::time::timeout(
        Duration::from_secs(600),
        runtime.stream_turn(turn_input, &sink, cancel.clone()),
    )
    .await
    .map_err(|_| {
        cancel.cancel();
        "librarian session timed out after 600s".to_string()
    })?
    .map_err(|e| format!("librarian turn failed: {e}"))?;

    Ok(turn.assistant_output.safe_text.trim().to_string())
}

fn build_runtime_services(
    project_id: i64,
    workspace_root: Option<&std::path::Path>,
    agent_id: &str,
) -> Result<RuntimeServices, String> {
    let tools: Arc<dyn ToolProvider> = Arc::new(LibrarianGraphTools { project_id });

    let instruction_source: Arc<dyn InstructionSource> = Arc::new(FsInstructionSource::new());
    let mut factories: Vec<Arc<dyn PluginFactory>> = vec![
        Arc::new(BuiltinToolResultProjectionPluginFactory::default()),
        Arc::new(StaticPluginFactory::new(
            "librarian_graph_tools",
            PluginSpec::new()
                .with_tool_provider(tools)
                .with_prompt_contributor(Arc::new(|_ctx| {
                    Box::pin(async { Ok(librarian_prompt_contributions()) })
                })),
        )),
    ];

    if workspace_root.is_some() {
        factories.push(Arc::new(ReadFilePluginFactory::new(Some(
            instruction_source,
        ))));
        factories.push(Arc::new(StaticPluginFactory::new(
            "glob",
            PluginSpec::new().with_tool_provider(Arc::new(LashGlob) as Arc<dyn ToolProvider>),
        )));
        factories.push(Arc::new(StaticPluginFactory::new(
            "grep",
            PluginSpec::new().with_tool_provider(Arc::new(LashGrep) as Arc<dyn ToolProvider>),
        )));
        factories.push(Arc::new(StaticPluginFactory::new(
            "ls",
            PluginSpec::new().with_tool_provider(Arc::new(LashLs) as Arc<dyn ToolProvider>),
        )));
    }

    let plugin_host = PluginHost::new(factories);
    let root_plugins = plugin_host
        .build_session(agent_id, default_execution_mode(), None)
        .map_err(|e| format!("failed to build librarian session: {e}"))?;
    Ok(RuntimeServices::new(root_plugins))
}

async fn resolve_project_workspace(project_id: i64) -> Option<PathBuf> {
    let store = ProjectStore::open().await.ok()?;
    let project = store.get_project(project_id).await.ok()?;
    if let Some(cwd) = project
        .shepherd_cwd
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        let path = PathBuf::from(cwd);
        if path.is_dir() {
            return Some(path);
        }
    }
    for ws in &project.workspaces {
        if let Some(path_str) = ws.path.as_deref().filter(|p| !p.trim().is_empty()) {
            let path = PathBuf::from(path_str);
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

struct NoopEventSink;

#[async_trait]
impl EventSink for NoopEventSink {
    async fn emit(&self, _event: SessionEvent) {}
}

// ── System prompt / prompt contributions ──

fn librarian_system_prompt(project_id: i64, workspace_root: Option<&std::path::Path>) -> String {
    let workspace_line = match workspace_root {
        Some(path) => format!("Workspace root: {}\n", path.display()),
        None => String::new(),
    };
    format!(
        "## Librarian Scope\n\n\
         You are the project Librarian for project {project_id}. You run as a one-shot background \
         job triggered by user activity or periodic lint. There is no persistent conversation — each \
         job starts fresh, so re-derive context from the knowledge graph every time.\n\n\
         {workspace_line}\
         Your only write surfaces are `apply_graph_patch` and `patch_canvas_document`. You have \
         read access to the graph (`search_graph`, `read_node`, `read_node_property`) and, when a \
         workspace is attached, workspace code (`ls`, `glob`, `grep`, `read_file`).\n\n\
         When you are done, summarize what changed in 1–3 sentences. If nothing was worth changing, \
         say so and stop.\n"
    )
}

fn librarian_prompt_contributions() -> Vec<PromptContribution> {
    vec![
        PromptContribution::guidance(
            "knowledge_graph_schema",
            "Knowledge Graph Schema",
            concat!(
                "## Node kinds\n\n",
                "| Kind | Purpose | ID convention |\n",
                "|------|---------|---------------|\n",
                "| `component` | Architectural building blocks | stable slug |\n",
                "| `entity` | Domain model objects | singular slug |\n",
                "| `convention` | How things should be done | topic slug |\n",
                "| `decision` | Why things are the way they are | descriptive slug |\n",
                "| `fact` | Project-specific truths, quirks, gotchas | descriptive slug |\n",
                "| `goal` | Active objectives or milestones | slug |\n",
                "| `document` | Free-form notes and project docs | slug |\n\n",
                "## Node fields\n\n",
                "- `kind`, `node_id`: identity (part of the record ID)\n",
                "- `label`: short human-readable title\n",
                "- `content`: the substance — useful to a future agent that knows nothing about the project\n",
                "- `subtype`: for `document` nodes, either `\"markdown\"` (default) or `\"html\"`. \
                   Markdown is the right choice for almost everything. Use `\"html\"` only when you need \
                   canvas-specific `<hirsel-*>` tags or rich interactive markup.\n",
                "- `tags`: flat string array for cross-cutting labels\n",
                "- `source`: `user` | `shepherd` | `librarian`\n",
                "- `metadata`: optional JSON for kind-specific structured data\n\n",
                "## Edge relations\n\n",
                "- `part_of`: structural containment (child → parent)\n",
                "- `depends_on`: runtime or build dependency\n",
                "- `implements`: component/entity that realizes a goal\n",
                "- `relates_to`: soft association\n",
            ),
        ),
        PromptContribution::guidance(
            "knowledge_graph_quality",
            "Knowledge Graph Quality",
            concat!(
                "Good graph content answers: \"What would a new agent need to know to work on this project effectively?\"\n\n",
                "- Prefer fewer, richer nodes over many thin ones.\n",
                "- Update existing nodes with new information rather than creating duplicates — always `search_graph` first.\n",
                "- Set `source = 'librarian'` on any node or edge you create.\n",
                "- Use `tags` for cross-cutting concerns instead of an edge per association.\n",
                "- For incremental edits to long `content`, use `patch_node_text` inside `apply_graph_patch`.\n",
                "\nBad: `label: \"Auth\"` content: `\"Handles authentication\"`\n",
                "Good: `label: \"Auth system\"` content: `\"JWT-based auth with RS256. Tokens expire 24h, ...\"`\n",
            ),
        ),
        PromptContribution::guidance(
            "graph_patch_tool",
            "apply_graph_patch",
            concat!(
                "`apply_graph_patch({ops, dry_run?})` is the only way to write the graph. Supported ops:\n\n",
                "- `upsert_node`: `{op: \"upsert_node\", kind, node_id, set: {label?, content?, subtype?, tags?, tags_add?, tags_remove?, metadata?, source?}}`\n",
                "- `upsert_edge`: `{op: \"upsert_edge\", from: {kind, node_id}, to: {kind, node_id}, relation, metadata?}`\n",
                "- `patch_node_text`: `{op: \"patch_node_text\", kind, node_id, field: \"content\", patch: \"*** Begin Patch ... *** End Patch\"}`\n",
                "- `delete_node`: `{op: \"delete_node\", kind, node_id}`\n",
                "- `delete_edge`: `{op: \"delete_edge\", from, to, relation}`\n\n",
                "Set `dry_run: true` to validate without writing. All endpoints use `{kind, node_id}` — no raw record IDs.\n\n",
                "For new `document` nodes, `subtype` defaults to `\"markdown\"`; set `\"html\"` only when you need \
                 canvas custom tags or interactive markup.\n\n",
                "Tags: prefer `tags_add` / `tags_remove` to adjust tags incrementally — they merge with the existing \
                 set. Use `tags` only when you want to replace the whole list. Tags are lowercased and deduped \
                 automatically.\n",
            ),
        ),
        PromptContribution::guidance(
            "index_maintenance",
            "Project Index",
            concat!(
                "The `document:index` node is the project map. Keep it current after material changes.\n",
                "Structure it by kind (Components, Entities, Conventions, Decisions, Facts, Goals) with inline ",
                "references like [component:auth] for each catalogued node.\n",
            ),
        ),
        PromptContribution::guidance(
            "canvas_patching",
            "patch_canvas_document",
            format!(
                "Use `patch_canvas_document(patch)` to edit the project canvas HTML in place. Patch format:\n\n{}\n\n\
                 Canvas-specific tags (`hirsel-node-ref`, `hirsel-node-field`, `hirsel-node-list`, \
                 `hirsel-doc-target`, `hirsel-doc-link`, `hirsel-doc-embed`) must use `node=\"kind:id\"` — \
                 no separate `kind`/`id` attributes and no `path=` on `hirsel-doc-link`.",
                TEXT_PATCH_INSTRUCTIONS,
            ),
        ),
    ]
}

// ── Tool provider ──

struct LibrarianGraphTools {
    project_id: i64,
}

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

#[async_trait]
impl ToolProvider for LibrarianGraphTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            tool_definition! {
                name: "search_graph".to_string(),
                description: "Full-text search across kg_node label and content fields. Returns compact node rows (no large content) ordered by relevance.".to_string(),
                params: vec![
                    ToolParam::typed("query", "str"),
                    ToolParam::optional("kinds", "list"),
                    ToolParam::optional("limit", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_node".to_string(),
                description: "Read one knowledge-graph node fully, including content, tags, metadata, and its incident edges.".to_string(),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("node_id", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_node_property".to_string(),
                description: format!(
                    "Read a text field of a node using a line range. Fields: {}. Max {MAX_LINES_PER_READ} lines per call.",
                    READABLE_TEXT_FIELDS.join(", ")
                ),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("node_id", "str"),
                    ToolParam::typed("field", "str"),
                    ToolParam::optional("start_line", "int"),
                    ToolParam::optional("end_line", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "apply_graph_patch".to_string(),
                description: "Apply bounded graph patch operations (upsert_node, upsert_edge, patch_node_text, delete_node, delete_edge). Set dry_run to validate without writing.".to_string(),
                params: vec![
                    ToolParam::typed("ops", "list"),
                    ToolParam::optional("dry_run", "bool"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "patch_canvas_document".to_string(),
                description: "Patch the project canvas HTML in place using the text patch format.".to_string(),
                params: vec![ToolParam::typed("patch", "str")],
                returns: "EditResult".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ]
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        match name {
            "search_graph" => self.search_graph(args).await,
            "read_node" => self.read_node(args).await,
            "read_node_property" => self.read_node_property(args).await,
            "apply_graph_patch" => self.apply_graph_patch(args).await,
            "patch_canvas_document" => self.patch_canvas_document(args).await,
            other => ToolResult::err(json!({ "error": format!("Unknown tool: {other}") })),
        }
    }
}

// ── Tool impls ──

impl LibrarianGraphTools {
    async fn search_graph(&self, args: &Value) -> ToolResult {
        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return ToolResult::err(json!({ "error": "query is required" }));
        };
        let kinds: Vec<String> = args
            .get("kinds")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, MAX_SEARCH_LIMIT as u64) as i64;

        let db = match librarian_db(self.project_id).await {
            Ok(db) => db,
            Err(error) => {
                return ToolResult::err(
                    json!({ "error": format!("failed to get librarian db: {error}") }),
                );
            }
        };

        let mut sql = String::from(
            "SELECT kind, node_id, label, summary, subtype, tags, source, \
             (search::score(1) + search::score(2)) AS score \
             FROM kg_node \
             WHERE (label @1@ $query OR content @2@ $query)",
        );
        if !kinds.is_empty() {
            sql.push_str(" AND kind IN $kinds");
        }
        sql.push_str(" ORDER BY score DESC LIMIT $limit;");

        let mut response = match db
            .query(sql)
            .bind(("query", query.to_string()))
            .bind(("kinds", kinds.clone()))
            .bind(("limit", limit))
            .await
        {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(
                    json!({ "error": format!("search_graph query failed: {error}") }),
                );
            }
        };
        let errors = response.take_errors();
        if !errors.is_empty() {
            return ToolResult::err(json!({
                "error": format_graph_errors(errors),
            }));
        }
        let rows: Vec<Value> = response.take(0).unwrap_or_default();
        ToolResult::ok(json!({
            "query": query,
            "limit": limit,
            "results": rows,
        }))
    }

    async fn read_node(&self, args: &Value) -> ToolResult {
        let Some(kind) = args.get("kind").and_then(Value::as_str) else {
            return ToolResult::err(json!({ "error": "kind is required" }));
        };
        let Some(node_id) = args.get("node_id").and_then(Value::as_str) else {
            return ToolResult::err(json!({ "error": "node_id is required" }));
        };
        let db = match librarian_db(self.project_id).await {
            Ok(db) => db,
            Err(error) => {
                return ToolResult::err(
                    json!({ "error": format!("failed to get librarian db: {error}") }),
                );
            }
        };

        let key = json!([self.project_id, kind, node_id]);
        let mut response = match db
            .query(
                "SELECT kind, node_id, label, summary, content, subtype, tags, source, metadata, updated_at \
                 FROM type::record('kg_node', $key);",
            )
            .bind(("key", key.clone()))
            .await
        {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(json!({ "error": format!("read_node failed: {error}") }));
            }
        };
        let node_rows: Vec<Value> = response.take(0).unwrap_or_default();
        let Some(node) = node_rows.into_iter().next() else {
            return ToolResult::err(json!({
                "error": format!("kg_node not found for {kind}:{node_id}")
            }));
        };

        let mut edge_response = match db
            .query(
                "SELECT relation, in AS in_record, out, metadata FROM kg_edge \
                 WHERE in = type::record('kg_node', $key) OR out = type::record('kg_node', $key);",
            )
            .bind(("key", key))
            .await
        {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(json!({ "error": format!("edge lookup failed: {error}") }));
            }
        };
        let edges: Vec<Value> = edge_response.take(0).unwrap_or_default();

        ToolResult::ok(json!({
            "node": node,
            "edges": edges,
        }))
    }

    async fn read_node_property(&self, args: &Value) -> ToolResult {
        let Some(kind) = args.get("kind").and_then(Value::as_str) else {
            return ToolResult::err(json!({ "error": "kind is required" }));
        };
        let Some(node_id) = args.get("node_id").and_then(Value::as_str) else {
            return ToolResult::err(json!({ "error": "node_id is required" }));
        };
        let Some(field) = args.get("field").and_then(Value::as_str) else {
            return ToolResult::err(json!({ "error": "field is required" }));
        };
        if !READABLE_TEXT_FIELDS.contains(&field) {
            return ToolResult::err(json!({
                "error": format!("field must be one of: {}", READABLE_TEXT_FIELDS.join(", "))
            }));
        }

        let start_line = args
            .get("start_line")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1) as usize;
        let end_line = args
            .get("end_line")
            .and_then(Value::as_u64)
            .map(|v| v as usize);

        let db = match librarian_db(self.project_id).await {
            Ok(db) => db,
            Err(error) => {
                return ToolResult::err(
                    json!({ "error": format!("failed to get librarian db: {error}") }),
                );
            }
        };
        let sql = format!(
            "SELECT {field} FROM type::record('kg_node', $key);",
            field = field
        );
        let mut response = match db
            .query(sql)
            .bind(("key", json!([self.project_id, kind, node_id])))
            .await
        {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(
                    json!({ "error": format!("read_node_property failed: {error}") }),
                );
            }
        };
        let rows: Vec<Value> = response.take(0).unwrap_or_default();
        let Some(row) = rows.into_iter().next() else {
            return ToolResult::err(json!({
                "error": format!("kg_node not found for {kind}:{node_id}")
            }));
        };
        let text = row
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let all_lines: Vec<&str> = text.lines().collect();
        let total = all_lines.len();
        let effective_end = end_line
            .unwrap_or(start_line + MAX_LINES_PER_READ - 1)
            .min(start_line + MAX_LINES_PER_READ - 1)
            .min(total.max(start_line));
        let slice_start = start_line.saturating_sub(1);
        let slice_end = effective_end.min(total);
        let mut out = String::new();
        if slice_start < slice_end {
            for (offset, line) in all_lines[slice_start..slice_end].iter().enumerate() {
                let line_no = slice_start + offset + 1;
                out.push_str(&format!("{line_no}: {line}\n"));
            }
        }

        ToolResult::ok(json!({
            "kind": kind,
            "node_id": node_id,
            "field": field,
            "total_lines": total,
            "range": { "start_line": start_line, "end_line": slice_end },
            "has_more": slice_end < total,
            "text": out,
        }))
    }

    async fn apply_graph_patch(&self, args: &Value) -> ToolResult {
        let ops = match args.get("ops") {
            Some(Value::Array(ops)) => ops.clone(),
            _ => {
                return ToolResult::err(json!({ "error": "ops must be an array" }));
            }
        };
        let dry_run = args
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let db = match librarian_db(self.project_id).await {
            Ok(db) => db,
            Err(error) => {
                return ToolResult::err(
                    json!({ "error": format!("failed to get librarian db: {error}") }),
                );
            }
        };

        let mut applied = Vec::new();
        let mut rejected = Vec::new();
        let mut mutated = false;

        for (idx, op) in ops.iter().enumerate() {
            let result = execute_patch_op(&db, self.project_id, op, dry_run).await;
            match result {
                Ok(value) => {
                    if !dry_run {
                        mutated = true;
                    }
                    applied.push(json!({ "index": idx, "result": value }));
                }
                Err(error) => rejected.push(json!({ "index": idx, "error": error })),
            }
        }

        if mutated {
            live_updates::publish_project(self.project_id, LiveUpdateKind::KnowledgeGraphChanged);
        }

        ToolResult::ok(json!({
            "dry_run": dry_run,
            "applied": applied,
            "rejected": rejected,
        }))
    }

    async fn patch_canvas_document(&self, args: &Value) -> ToolResult {
        let Some(patch) = args.get("patch").and_then(Value::as_str) else {
            return ToolResult::err(json!({ "error": "patch is required" }));
        };
        let current_document = match documents::get_canvas_document(self.project_id).await {
            Ok(Some(document)) => document,
            Ok(None) => {
                return ToolResult::err(json!({ "error": "document:canvas does not exist" }));
            }
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("failed to load canvas document: {error}")
                }));
            }
        };

        let patched = match apply_text_patch(&current_document.html, patch) {
            Ok(outcome) => outcome,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        if let Err(message) = validate_canvas_markup_contract(&patched.new_text) {
            return ToolResult::err(json!({ "error": message }));
        }

        let patched_html = patched.new_text.clone();
        let result =
            documents::upsert_canvas_document(self.project_id, &patched_html, Some("librarian"))
                .await;

        match result {
            Ok(document) => {
                live_updates::publish_project(
                    self.project_id,
                    LiveUpdateKind::KnowledgeGraphChanged,
                );
                let mut fields = Map::new();
                fields.insert("project_id".to_string(), json!(self.project_id));
                fields.insert("node_id".to_string(), json!(document.node_id));
                fields.insert("added".to_string(), json!(patched.added_lines));
                fields.insert("removed".to_string(), json!(patched.removed_lines));
                fields.insert("updated_at".to_string(), json!(document.updated_at));
                fields.insert(
                    "__type__".to_string(),
                    Value::String("edit_result".to_string()),
                );
                fields.insert(
                    "summary".to_string(),
                    Value::String("Patched canvas document".to_string()),
                );
                ToolResult::ok(Value::Object(fields))
            }
            Err(errors) => ToolResult::err(json!({
                "error": format_canvas_validation_errors(&errors),
                "details": errors,
            })),
        }
    }
}

// ── apply_graph_patch op handlers ──

async fn execute_patch_op(
    db: &crate::backend::db::DbClient,
    project_id: i64,
    op: &Value,
    dry_run: bool,
) -> Result<Value, String> {
    let op_name = op
        .get("op")
        .and_then(Value::as_str)
        .ok_or("op is required")?;
    match op_name {
        "upsert_node" => upsert_node(db, project_id, op, dry_run).await,
        "upsert_edge" => upsert_edge(db, project_id, op, dry_run).await,
        "patch_node_text" => patch_node_text(db, project_id, op, dry_run).await,
        "delete_node" => delete_node(db, project_id, op, dry_run).await,
        "delete_edge" => delete_edge(db, project_id, op, dry_run).await,
        other => Err(format!("unknown op: {other}")),
    }
}

fn endpoint_key(project_id: i64, op: &Value, field: &str) -> Result<Value, String> {
    let endpoint = op
        .get(field)
        .ok_or_else(|| format!("{field} is required"))?;
    let kind = endpoint
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field}.kind is required"))?;
    let node_id = endpoint
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field}.node_id is required"))?;
    Ok(json!([project_id, kind, node_id]))
}

async fn upsert_node(
    db: &crate::backend::db::DbClient,
    project_id: i64,
    op: &Value,
    dry_run: bool,
) -> Result<Value, String> {
    let kind = op
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("kind is required")?
        .to_string();
    let node_id = op
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or("node_id is required")?
        .to_string();
    let empty = Map::new();
    let set = op.get("set").and_then(Value::as_object).unwrap_or(&empty);
    let mut merge = Map::new();
    merge.insert("kind".to_string(), json!(kind));
    merge.insert("node_id".to_string(), json!(node_id));
    merge.insert(
        "source".to_string(),
        set.get("source")
            .cloned()
            .unwrap_or_else(|| json!("librarian")),
    );
    for field in ["label", "summary", "content", "metadata"] {
        if let Some(value) = set.get(field) {
            merge.insert(field.to_string(), value.clone());
        }
    }
    // Documents carry a subtype that controls how the UI renders their content.
    if kind == "document" {
        let normalized = normalize_document_subtype(set.get("subtype").and_then(Value::as_str));
        merge.insert("subtype".to_string(), json!(normalized));
    }

    // Tags: `tags` replaces wholesale; `tags_add` / `tags_remove` apply incrementally.
    let key = json!([project_id, kind, node_id]);
    let tags_add = tag_list_from_set(set, "tags_add");
    let tags_remove = tag_list_from_set(set, "tags_remove");
    let tags_replace = set
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| collect_tag_strings(arr));
    if let Some(replaced) = tags_replace {
        merge.insert("tags".to_string(), json!(normalize_tag_list(replaced)));
    } else if !tags_add.is_empty() || !tags_remove.is_empty() {
        let existing = if dry_run {
            Vec::new()
        } else {
            load_existing_tags(db, &key).await.unwrap_or_default()
        };
        let next = apply_tag_delta(existing, &tags_add, &tags_remove);
        merge.insert("tags".to_string(), json!(next));
    }

    if dry_run {
        return Ok(json!({
            "op": "upsert_node",
            "status": "dry_run",
            "kind": kind,
            "node_id": node_id,
            "set": merge,
        }));
    }

    db.query("UPSERT type::record('kg_node', $key) MERGE $merge RETURN id;")
        .bind(("key", key))
        .bind(("merge", Value::Object(merge)))
        .await
        .map_err(|e| format!("upsert_node failed: {e}"))?;
    Ok(json!({
        "op": "upsert_node",
        "status": "applied",
        "kind": kind,
        "node_id": node_id,
    }))
}

fn tag_list_from_set(set: &Map<String, Value>, field: &str) -> Vec<String> {
    set.get(field)
        .and_then(Value::as_array)
        .map(|arr| collect_tag_strings(arr))
        .unwrap_or_default()
}

fn collect_tag_strings(arr: &[Value]) -> Vec<String> {
    arr.iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn normalize_tag_list(tags: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(tags.len());
    for tag in tags {
        let normalized = tag.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

fn apply_tag_delta(existing: Vec<String>, add: &[String], remove: &[String]) -> Vec<String> {
    let remove_set: std::collections::HashSet<String> = remove
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .collect();
    let mut next: Vec<String> = existing
        .into_iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty() && !remove_set.contains(t))
        .collect();
    for tag in add {
        let normalized = tag.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if !next.iter().any(|t| t == &normalized) {
            next.push(normalized);
        }
    }
    normalize_tag_list(next)
}

async fn load_existing_tags(
    db: &crate::backend::db::DbClient,
    key: &Value,
) -> Result<Vec<String>, String> {
    let mut response = db
        .query("SELECT tags FROM type::record('kg_node', $key);")
        .bind(("key", key.clone()))
        .await
        .map_err(|e| format!("failed to load tags: {e}"))?;
    let rows: Vec<Value> = response.take(0).unwrap_or_default();
    Ok(rows
        .into_iter()
        .next()
        .and_then(|row| row.get("tags").and_then(Value::as_array).cloned())
        .map(|arr| collect_tag_strings(&arr))
        .unwrap_or_default())
}

async fn upsert_edge(
    db: &crate::backend::db::DbClient,
    project_id: i64,
    op: &Value,
    dry_run: bool,
) -> Result<Value, String> {
    let from_key = endpoint_key(project_id, op, "from")?;
    let to_key = endpoint_key(project_id, op, "to")?;
    let relation = op
        .get("relation")
        .and_then(Value::as_str)
        .ok_or("relation is required")?
        .to_string();
    let metadata = op
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .map(Value::Object)
        .unwrap_or(Value::Object(Map::new()));

    if dry_run {
        return Ok(json!({
            "op": "upsert_edge",
            "status": "dry_run",
            "from": from_key,
            "to": to_key,
            "relation": relation,
        }));
    }

    db.query(
        "DELETE kg_edge WHERE in = type::record('kg_node', $from_key) \
         AND out = type::record('kg_node', $to_key) AND relation = $relation; \
         RELATE type::record('kg_node', $from_key) -> kg_edge -> type::record('kg_node', $to_key) \
         SET relation = $relation, metadata = $metadata;",
    )
    .bind(("from_key", from_key.clone()))
    .bind(("to_key", to_key.clone()))
    .bind(("relation", relation.clone()))
    .bind(("metadata", metadata))
    .await
    .map_err(|e| format!("upsert_edge failed: {e}"))?;

    Ok(json!({
        "op": "upsert_edge",
        "status": "applied",
        "from": from_key,
        "to": to_key,
        "relation": relation,
    }))
}

async fn patch_node_text(
    db: &crate::backend::db::DbClient,
    project_id: i64,
    op: &Value,
    dry_run: bool,
) -> Result<Value, String> {
    let kind = op
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("kind is required")?
        .to_string();
    let node_id = op
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or("node_id is required")?
        .to_string();
    let field = op
        .get("field")
        .and_then(Value::as_str)
        .ok_or("field is required")?
        .to_string();
    if !WRITABLE_TEXT_FIELDS.contains(&field.as_str()) {
        return Err(format!(
            "field must be one of: {}",
            WRITABLE_TEXT_FIELDS.join(", ")
        ));
    }
    let patch = op
        .get("patch")
        .and_then(Value::as_str)
        .ok_or("patch is required")?;

    let key = json!([project_id, kind, node_id]);
    let select_sql = format!(
        "SELECT {field} FROM type::record('kg_node', $key);",
        field = field
    );
    let mut response = db
        .query(select_sql)
        .bind(("key", key.clone()))
        .await
        .map_err(|e| format!("failed to load node for patch: {e}"))?;
    let rows: Vec<Value> = response.take(0).unwrap_or_default();
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| format!("kg_node not found for {kind}:{node_id}"))?;
    let current = row
        .get(&field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let outcome = apply_text_patch(&current, patch)?;

    if dry_run {
        return Ok(json!({
            "op": "patch_node_text",
            "status": "dry_run",
            "kind": kind,
            "node_id": node_id,
            "field": field,
            "added": outcome.added_lines,
            "removed": outcome.removed_lines,
        }));
    }

    let update_sql = format!(
        "UPDATE type::record('kg_node', $key) MERGE {{ {field}: $value }};",
        field = field
    );
    db.query(update_sql)
        .bind(("key", key))
        .bind(("value", outcome.new_text))
        .await
        .map_err(|e| format!("patch_node_text update failed: {e}"))?;

    Ok(json!({
        "op": "patch_node_text",
        "status": "applied",
        "kind": kind,
        "node_id": node_id,
        "field": field,
        "added": outcome.added_lines,
        "removed": outcome.removed_lines,
    }))
}

async fn delete_node(
    db: &crate::backend::db::DbClient,
    project_id: i64,
    op: &Value,
    dry_run: bool,
) -> Result<Value, String> {
    let kind = op
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("kind is required")?
        .to_string();
    let node_id = op
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or("node_id is required")?
        .to_string();
    let key = json!([project_id, kind, node_id]);

    if dry_run {
        return Ok(json!({
            "op": "delete_node",
            "status": "dry_run",
            "kind": kind,
            "node_id": node_id,
        }));
    }

    db.query(
        "DELETE kg_edge WHERE in = type::record('kg_node', $key) OR out = type::record('kg_node', $key); \
         DELETE type::record('kg_node', $key);",
    )
    .bind(("key", key))
    .await
    .map_err(|e| format!("delete_node failed: {e}"))?;

    Ok(json!({
        "op": "delete_node",
        "status": "applied",
        "kind": kind,
        "node_id": node_id,
    }))
}

async fn delete_edge(
    db: &crate::backend::db::DbClient,
    project_id: i64,
    op: &Value,
    dry_run: bool,
) -> Result<Value, String> {
    let from_key = endpoint_key(project_id, op, "from")?;
    let to_key = endpoint_key(project_id, op, "to")?;
    let relation = op
        .get("relation")
        .and_then(Value::as_str)
        .ok_or("relation is required")?
        .to_string();

    if dry_run {
        return Ok(json!({
            "op": "delete_edge",
            "status": "dry_run",
            "from": from_key,
            "to": to_key,
            "relation": relation,
        }));
    }

    db.query(
        "DELETE kg_edge WHERE in = type::record('kg_node', $from_key) \
         AND out = type::record('kg_node', $to_key) AND relation = $relation;",
    )
    .bind(("from_key", from_key.clone()))
    .bind(("to_key", to_key.clone()))
    .bind(("relation", relation.clone()))
    .await
    .map_err(|e| format!("delete_edge failed: {e}"))?;

    Ok(json!({
        "op": "delete_edge",
        "status": "applied",
        "from": from_key,
        "to": to_key,
        "relation": relation,
    }))
}

// ── Canvas validation + helpers ──

fn validate_canvas_markup_contract(html: &str) -> Result<(), String> {
    let fragment = scraper::Html::parse_fragment(html);
    for tag_name in CANVAS_TAGS_REQUIRING_NODE_ATTR {
        let selector = scraper::Selector::parse(tag_name)
            .map_err(|error| format!("invalid selector for {tag_name}: {error}"))?;
        for node in fragment.select(&selector) {
            let attrs = node.value();
            let node_attr = attrs.attr("node").map(str::trim).unwrap_or("");
            if node_attr.is_empty() {
                return Err(format!("<{tag_name}> requires node=\"kind:id\""));
            }
            let Some((kind, node_id)) = node_attr.split_once(':') else {
                return Err(format!("<{tag_name}> node attribute must be kind:id"));
            };
            if kind.trim().is_empty() || node_id.trim().is_empty() {
                return Err(format!("<{tag_name}> node attribute must be kind:id"));
            }
            if attrs.attr("kind").is_some() || attrs.attr("id").is_some() {
                return Err(format!(
                    "<{tag_name}> must not include separate kind/id attributes"
                ));
            }
            if *tag_name == "hirsel-doc-link" && attrs.attr("path").is_some() {
                return Err(
                    "<hirsel-doc-link> must not use path=; use node=\"kind:id\"".to_string()
                );
            }
        }
    }
    Ok(())
}

fn format_canvas_validation_errors(errors: &[DocumentValidationError]) -> String {
    errors
        .iter()
        .map(|error| error.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Validate a document subtype, defaulting to `"markdown"` for missing or unknown values.
/// Only `"markdown"` and `"html"` are accepted.
fn normalize_document_subtype(raw: Option<&str>) -> String {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("html") => "html".to_string(),
        _ => "markdown".to_string(),
    }
}

pub(crate) fn query_might_mutate_graph(query: &str) -> bool {
    let upper = query.to_uppercase();
    GRAPH_MUTATION_KEYWORDS
        .iter()
        .any(|keyword| upper.contains(keyword))
}

fn format_graph_errors(errors: std::collections::HashMap<usize, surrealdb::Error>) -> String {
    errors
        .into_iter()
        .map(|(index, error)| format!("statement {index}: {error}"))
        .collect::<Vec<_>>()
        .join("\n")
}
