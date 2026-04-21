//! Background worker that turns `kg_node` content changes into
//! `kg_node_chunk` rows.
//!
//! Triggered by `enqueue_chunk_job` (called from the `apply_graph_patch`
//! op handlers whenever a node's `content` is written). Processes jobs
//! one at a time: load node → split → contextualise → batch-embed →
//! delete-then-insert chunks for that node.
//!
//! No local fallback. If the OpenRouter key is unset, every call errors
//! and the worker backs off until someone configures the key.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use surrealdb::types::{RecordId, SurrealValue};
use tokio::time::sleep;

use crate::backend::chunking::{self, ChunkingConfig, CHUNKER_VERSION};
use crate::backend::db::{global_db, DbClient};
use crate::backend::embeddings::{EmbeddingClient, CONTEXTUALIZER_VERSION, EMBEDDING_VERSION};
use crate::backend::job_queue;
use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

const IDLE_BACKOFF: Duration = Duration::from_secs(2);
const NOKEY_BACKOFF: Duration = Duration::from_secs(60);
const ERROR_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Deserialize, SurrealValue)]
struct ChunkJobRow {
    id: RecordId,
    project_id: i64,
    node_kind: String,
    node_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    status: String,
    #[serde(default)]
    #[allow(dead_code)]
    last_error: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    created_at: Option<DateTime<Utc>>,
}

/// Enqueue a chunk job for a node whose `content` has just been written.
/// Idempotent: if a queued job already exists for `(project, kind, node_id)`,
/// this is a no-op.
pub async fn enqueue_chunk_job(
    project_id: i64,
    node_kind: &str,
    node_id: &str,
) -> Result<(), String> {
    let db = global_db().await;
    db.query(
        "LET $existing = (SELECT id FROM kg_chunk_job WHERE \
           project_id = $pid AND node_kind = $kind AND node_id = $node_id \
           AND status IN ['queued', 'running'] LIMIT 1); \
         IF array::len($existing) = 0 THEN \
           CREATE kg_chunk_job CONTENT { \
             project_id: $pid, node_kind: $kind, node_id: $node_id, status: 'queued' \
           } \
         END;",
    )
    .bind(("pid", project_id))
    .bind(("kind", node_kind.to_string()))
    .bind(("node_id", node_id.to_string()))
    .await
    .map_err(|e| format!("enqueue_chunk_job: {e}"))?;
    Ok(())
}

async fn claim_next(db: &DbClient) -> Result<Option<ChunkJobRow>, String> {
    // SurrealDB's `LET` statements don't contribute a response group and
    // SurrealValue deserialization of a `take(N)` off the wrong index can
    // silently return an empty Vec. The old LET/UPDATE/SELECT pattern
    // flipped rows to "running" but left the reader empty — every claim
    // orphaned a job. Two plain queries avoid the index hazard entirely.
    let mut select_response = db
        .query(
            "SELECT * FROM kg_chunk_job WHERE status = 'queued' \
             ORDER BY created_at ASC LIMIT 1",
        )
        .await
        .map_err(|e| format!("select queued chunk job: {e}"))?;
    let rows: Vec<ChunkJobRow> = select_response.take(0).unwrap_or_default();
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    db.query("UPDATE $id SET status = 'running'")
        .bind(("id", row.id.clone()))
        .await
        .map_err(|e| format!("mark chunk job running: {e}"))?;
    Ok(Some(row))
}

#[derive(Debug, Deserialize, SurrealValue)]
struct NodeContentRow {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
}

async fn load_node_content(
    db: &DbClient,
    project_id: i64,
    kind: &str,
    node_id: &str,
) -> Result<Option<(String, Option<String>)>, String> {
    let key = json!([project_id, kind, node_id]);
    let mut response = db
        .query("SELECT content, subtype FROM type::record('kg_node', $key);")
        .bind(("key", key))
        .await
        .map_err(|e| format!("load node content: {e}"))?;
    let rows: Vec<NodeContentRow> = response.take(0).unwrap_or_default();
    match rows.into_iter().next() {
        None => Ok(None),
        Some(row) => match row.content {
            Some(c) if !c.trim().is_empty() => Ok(Some((c, row.subtype))),
            _ => Ok(None),
        },
    }
}

/// Delete every chunk for this node and insert the new batch. Called
/// under the same worker turn so there is no dangling-generation window
/// beyond the individual writes.
async fn replace_chunks(
    db: &DbClient,
    project_id: i64,
    kind: &str,
    node_id: &str,
    chunks: &[IndexedChunk],
    embedding_model: &str,
) -> Result<(), String> {
    db.query(
        "DELETE kg_node_chunk WHERE project_id = $pid AND node_kind = $kind AND node_id = $node_id;",
    )
    .bind(("pid", project_id))
    .bind(("kind", kind.to_string()))
    .bind(("node_id", node_id.to_string()))
    .await
    .map_err(|e| format!("delete existing chunks: {e}"))?;

    for chunk in chunks {
        db.query(
            "CREATE kg_node_chunk CONTENT { \
               project_id: $pid, node_kind: $kind, node_id: $node_id, \
               chunk_index: $chunk_index, chunk_path: $chunk_path, \
               chunk_text: $chunk_text, context_text: $context_text, \
               fused_text: $fused_text, embedding: $embedding, \
               token_count: $token_count, embedding_model: $embedding_model, \
               chunker_version: $chunker_version \
             };",
        )
        .bind(("pid", project_id))
        .bind(("kind", kind.to_string()))
        .bind(("node_id", node_id.to_string()))
        .bind(("chunk_index", chunk.chunk_index as i64))
        .bind(("chunk_path", chunk.chunk_path.clone()))
        .bind(("chunk_text", chunk.chunk_text.clone()))
        .bind(("context_text", chunk.context_text.clone()))
        .bind(("fused_text", chunk.fused_text.clone()))
        .bind(("embedding", chunk.embedding.clone()))
        .bind(("token_count", chunk.token_count as i64))
        .bind(("embedding_model", embedding_model.to_string()))
        .bind(("chunker_version", CHUNKER_VERSION.to_string()))
        .await
        .map_err(|e| format!("insert chunk: {e}"))?;
    }
    Ok(())
}

struct IndexedChunk {
    chunk_index: usize,
    chunk_path: Vec<String>,
    chunk_text: String,
    context_text: String,
    fused_text: String,
    token_count: usize,
    embedding: Vec<f32>,
}

async fn process_job(
    db: &DbClient,
    client: &EmbeddingClient,
    cfg: &ChunkingConfig,
    job: &ChunkJobRow,
) -> Result<(), String> {
    let loaded = load_node_content(db, job.project_id, &job.node_kind, &job.node_id).await?;
    let (content, subtype) = match loaded {
        Some(pair) => pair,
        None => {
            // Node has no content (or doesn't exist any more). Wipe any
            // stale chunks and finish.
            db.query(
                "DELETE kg_node_chunk WHERE project_id = $pid AND node_kind = $kind AND node_id = $node_id;",
            )
            .bind(("pid", job.project_id))
            .bind(("kind", job.node_kind.clone()))
            .bind(("node_id", job.node_id.clone()))
            .await
            .map_err(|e| format!("delete stale chunks: {e}"))?;
            return Ok(());
        }
    };

    let drafts = chunking::split_into_chunks(&content, subtype.as_deref(), cfg);
    if drafts.is_empty() {
        db.query(
            "DELETE kg_node_chunk WHERE project_id = $pid AND node_kind = $kind AND node_id = $node_id;",
        )
        .bind(("pid", job.project_id))
        .bind(("kind", job.node_kind.clone()))
        .bind(("node_id", job.node_id.clone()))
        .await
        .map_err(|e| format!("delete chunks (empty split): {e}"))?;
        return Ok(());
    }

    // Contextualise each chunk in parallel-friendly order.
    let mut contexts: Vec<String> = Vec::with_capacity(drafts.len());
    for draft in &drafts {
        let ctx = client
            .contextualize_chunk(&content, &draft.chunk_text, &draft.chunk_path)
            .await?;
        contexts.push(ctx);
    }

    let fused: Vec<String> = drafts
        .iter()
        .zip(contexts.iter())
        .map(|(d, c)| format!("{c}\n\n{}", d.chunk_text))
        .collect();

    let embeddings = client.embed_texts(&fused).await?;
    if embeddings.len() != drafts.len() {
        return Err(format!(
            "embedding count mismatch: {} drafts vs {} embeddings",
            drafts.len(),
            embeddings.len()
        ));
    }

    let mut indexed: Vec<IndexedChunk> = Vec::with_capacity(drafts.len());
    for ((draft, context_text), (fused_text, embedding)) in drafts
        .into_iter()
        .zip(contexts)
        .zip(fused.into_iter().zip(embeddings))
    {
        indexed.push(IndexedChunk {
            chunk_index: draft.chunk_index,
            chunk_path: draft.chunk_path,
            chunk_text: draft.chunk_text,
            context_text,
            fused_text,
            token_count: draft.token_count,
            embedding,
        });
    }

    let embedding_model =
        RuntimeSettings::get_or(keys::EMBEDDING_MODEL, Defaults::EMBEDDING_MODEL.to_string()).await;
    replace_chunks(
        db,
        job.project_id,
        &job.node_kind,
        &job.node_id,
        &indexed,
        &embedding_model,
    )
    .await?;

    tracing::debug!(
        project_id = job.project_id,
        kind = %job.node_kind,
        node_id = %job.node_id,
        chunks = indexed.len(),
        embedding_version = EMBEDDING_VERSION,
        contextualizer_version = CONTEXTUALIZER_VERSION,
        "chunked node"
    );
    Ok(())
}

/// Spawn the background chunking worker. Loops forever; parks on
/// missing API key; one job at a time to keep OpenRouter spend bounded.
pub fn spawn_worker() {
    tokio::spawn(async move {
        let db = global_db().await;
        if let Err(error) = job_queue::requeue_running(db, "kg_chunk_job").await {
            tracing::warn!(%error, "failed to requeue stale chunk jobs");
        }

        loop {
            // Pick up config fresh each loop so runtime overrides take
            // effect without restarting.
            let cfg = ChunkingConfig::from_settings().await;

            let client = match EmbeddingClient::from_credentials().await {
                Ok(c) => c,
                Err(error) => {
                    tracing::debug!(%error, "chunk worker idle (no OpenRouter key)");
                    sleep(NOKEY_BACKOFF).await;
                    continue;
                }
            };

            match claim_next(db).await {
                Ok(Some(job)) => {
                    let job_id = job.id.clone();
                    let project_id = job.project_id;
                    let kind = job.node_kind.clone();
                    let node_id = job.node_id.clone();
                    match process_job(db, &client, &cfg, &job).await {
                        Ok(_) => {
                            if let Err(e) = job_queue::mark_finished(db, &job_id).await {
                                tracing::warn!(%e, "failed to mark chunk job finished");
                            } else {
                                tracing::debug!(project_id, %kind, %node_id, "chunk job completed");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, project_id, %kind, %node_id, "chunk job failed");
                            let _ = job_queue::mark_failed(db, &job_id, &error).await;
                            sleep(ERROR_BACKOFF).await;
                        }
                    }
                }
                Ok(None) => sleep(IDLE_BACKOFF).await,
                Err(error) => {
                    tracing::warn!(%error, "claim chunk job failed");
                    sleep(ERROR_BACKOFF).await;
                }
            }
        }
    });
}
