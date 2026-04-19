//! Subgraph chunking. Splits a knowledge-graph neighbourhood rooted at a
//! given node into token-bounded coherent pieces, preserving cross-chunk
//! edges so consumers know there's more context next door.
//!
//! Consumer pattern (coordinator thread):
//!   chunks = graph.chunk_subgraph(root_kind, root_id)
//!   handles = spawn_thread_batch([
//!     { objective: "summarise chunk {i}", binding_data: chunk_json }
//!     for chunk in chunks
//!   ])
//!   summaries = await_threads(handles)
//!   merged = spawn_thread({ objective: "merge", inputs: summaries })

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use super::db::global_db;
use super::runtime_settings::{keys, Defaults, RuntimeSettings};

/// A node as emitted in a chunk. Carries a content preview (truncated to
/// avoid bloating the chunk itself) — consumers can re-fetch full content
/// by node id if they want the whole thing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkNode {
    pub kind: String,
    pub node_id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
    pub approx_tokens: usize,
}

/// Edge within (or crossing) a chunk. `from` and `to` use `"kind:id"` for
/// compactness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEdge {
    pub from: String,
    pub relation: String,
    pub to: String,
}

/// One chunk of the subgraph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub index: usize,
    pub nodes: Vec<ChunkNode>,
    pub edges: Vec<ChunkEdge>,
    /// Edges whose endpoints land in different chunks. Consumers use these
    /// to know "there's more relevant context in chunk N".
    pub cross_refs: Vec<CrossRef>,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRef {
    pub from: String,
    pub relation: String,
    pub to: String,
    /// The chunk index the far endpoint landed in.
    pub other_chunk: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),
    #[error("Root node not found: {kind}:{node_id}")]
    RootNotFound { kind: String, node_id: String },
}

/// Entry point. Reads `graph.chunk.max_tokens` default when `max_tokens`
/// is `None`.
pub async fn chunk_subgraph(
    project_id: i64,
    root_kind: &str,
    root_id: &str,
    max_tokens: Option<usize>,
) -> Result<Vec<Chunk>, ChunkError> {
    let budget = RuntimeSettings::resolve(
        keys::GRAPH_CHUNK_MAX_TOKENS,
        max_tokens,
        Defaults::GRAPH_CHUNK_MAX_TOKENS,
    )
    .await;
    chunk_subgraph_bounded(project_id, root_kind, root_id, budget).await
}

async fn chunk_subgraph_bounded(
    project_id: i64,
    root_kind: &str,
    root_id: &str,
    max_tokens: usize,
) -> Result<Vec<Chunk>, ChunkError> {
    let db = global_db().await;

    // Verify root exists and fetch the initial node.
    let root =
        load_node(project_id, root_kind, root_id)
            .await?
            .ok_or(ChunkError::RootNotFound {
                kind: root_kind.to_string(),
                node_id: root_id.to_string(),
            })?;

    let mut queue: VecDeque<ChunkNode> = VecDeque::new();
    queue.push_back(root);

    let mut visited: HashSet<String> = HashSet::new();
    // kind:id -> chunk index
    let mut node_chunk: HashMap<String, usize> = HashMap::new();
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current_nodes: Vec<ChunkNode> = Vec::new();
    let mut current_edges: Vec<ChunkEdge> = Vec::new();
    let mut current_tokens: usize = 0;
    // Pending edges indexed by both endpoints so we can classify them once
    // both nodes are placed. Vec<(from, relation, to)>.
    let mut pending_edges: Vec<(String, String, String)> = Vec::new();

    while let Some(node) = queue.pop_front() {
        let key = node_key(&node.kind, &node.node_id);
        if visited.contains(&key) {
            continue;
        }
        visited.insert(key.clone());

        // If adding this node would exceed the budget and the chunk isn't
        // empty, seal the current chunk and open a fresh one.
        if !current_nodes.is_empty() && current_tokens + node.approx_tokens > max_tokens {
            let idx = chunks.len();
            chunks.push(Chunk {
                index: idx,
                nodes: std::mem::take(&mut current_nodes),
                edges: std::mem::take(&mut current_edges),
                cross_refs: Vec::new(),
                total_tokens: current_tokens,
            });
            current_tokens = 0;
        }

        current_tokens += node.approx_tokens;
        let chunk_idx = chunks.len();
        node_chunk.insert(key.clone(), chunk_idx);
        current_nodes.push(node.clone());

        // Enqueue outgoing neighbours.
        let edges = outgoing_edges(&db, project_id, &node.kind, &node.node_id).await?;
        for (relation, to_kind, to_id) in edges {
            let to_key = node_key(&to_kind, &to_id);
            pending_edges.push((key.clone(), relation.clone(), to_key.clone()));
            if !visited.contains(&to_key) {
                if let Ok(Some(next)) = load_node(project_id, &to_kind, &to_id).await {
                    queue.push_back(next);
                }
            }
        }
    }

    // Seal the final chunk.
    if !current_nodes.is_empty() {
        let idx = chunks.len();
        chunks.push(Chunk {
            index: idx,
            nodes: current_nodes,
            edges: current_edges,
            cross_refs: Vec::new(),
            total_tokens: current_tokens,
        });
    }

    // Classify pending edges.
    for (from, relation, to) in pending_edges {
        let from_chunk = match node_chunk.get(&from) {
            Some(c) => *c,
            None => continue,
        };
        let to_chunk = match node_chunk.get(&to) {
            Some(c) => *c,
            None => continue,
        };
        if from_chunk == to_chunk {
            chunks[from_chunk].edges.push(ChunkEdge {
                from: from.clone(),
                relation: relation.clone(),
                to: to.clone(),
            });
        } else {
            chunks[from_chunk].cross_refs.push(CrossRef {
                from: from.clone(),
                relation: relation.clone(),
                to: to.clone(),
                other_chunk: to_chunk,
            });
            chunks[to_chunk].cross_refs.push(CrossRef {
                from,
                relation,
                to,
                other_chunk: from_chunk,
            });
        }
    }

    Ok(chunks)
}

fn node_key(kind: &str, node_id: &str) -> String {
    format!("{kind}:{node_id}")
}

/// Approximate token count ≈ chars / 4. Deliberately coarse; structural
/// chunking only needs bounded size, not exact tokens.
fn approx_tokens(s: &str) -> usize {
    s.chars().count() / 4 + 1
}

async fn load_node(
    project_id: i64,
    kind: &str,
    node_id: &str,
) -> Result<Option<ChunkNode>, ChunkError> {
    let db = global_db().await;
    #[derive(Deserialize, SurrealValue)]
    struct Row {
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default)]
        tags: Option<Vec<String>>,
    }
    let mut response = db
        .query("SELECT label, content, subtype, tags FROM type::record('kg_node', [$pid, $kind, $nid])")
        .bind(("pid", project_id))
        .bind(("kind", kind.to_string()))
        .bind(("nid", node_id.to_string()))
        .await?;
    let rows: Vec<Row> = response.take(0).unwrap_or_default();
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let label = row.label.unwrap_or_default();
    let content = row.content.unwrap_or_default();
    let preview: Option<String> = if content.is_empty() {
        None
    } else {
        let snippet: String = content.chars().take(400).collect();
        Some(if snippet.len() < content.len() {
            format!("{snippet}…")
        } else {
            snippet
        })
    };
    let tokens = approx_tokens(&label) + approx_tokens(&content);
    Ok(Some(ChunkNode {
        kind: kind.to_string(),
        node_id: node_id.to_string(),
        label,
        content_preview: preview,
        subtype: row.subtype,
        tags: row.tags.unwrap_or_default(),
        approx_tokens: tokens,
    }))
}

async fn outgoing_edges(
    db: &super::db::DbClient,
    project_id: i64,
    kind: &str,
    node_id: &str,
) -> Result<Vec<(String, String, String)>, ChunkError> {
    // `kg_edge` uses composite RecordIds [project_id, kind, node_id] on
    // both endpoints, matching how `kg_node` records are keyed.
    let mut response = db
        .query(
            "SELECT relation, `in` AS in_rec, out AS out_rec FROM kg_edge \
             WHERE `in` = type::record('kg_node', [$pid, $kind, $nid])",
        )
        .bind(("pid", project_id))
        .bind(("kind", kind.to_string()))
        .bind(("nid", node_id.to_string()))
        .await?;
    #[derive(Deserialize, SurrealValue)]
    struct Row {
        #[serde(default)]
        relation: String,
        #[serde(default)]
        out_rec: Option<surrealdb::types::RecordId>,
    }
    let rows: Vec<Row> = response.take(0).unwrap_or_default();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(rid) = row.out_rec else { continue };
        if let Some((to_kind, to_id)) = parse_kg_record_id(&rid) {
            out.push((row.relation, to_kind, to_id));
        }
    }
    Ok(out)
}

/// `kg_node:⟨[project_id, kind, node_id]⟩` → `(kind, id)`.
fn parse_kg_record_id(rid: &surrealdb::types::RecordId) -> Option<(String, String)> {
    // RecordId's key is composite; serialize to JSON to extract fields.
    let json = serde_json::to_value(rid).ok()?;
    let array = json.get("id")?.as_array()?;
    if array.len() < 3 {
        return None;
    }
    let kind = array.get(1)?.as_str()?.to_string();
    let node_id = array.get(2)?.as_str()?.to_string();
    Some((kind, node_id))
}

#[cfg(test)]
mod tests {
    use super::approx_tokens;

    #[test]
    fn token_count_rough() {
        assert!(approx_tokens("hello") >= 1);
        assert!(approx_tokens("a".repeat(400).as_str()) >= 100);
    }
}
