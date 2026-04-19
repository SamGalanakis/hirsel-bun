-- Knowledge-graph node chunk store.
--
-- One row per coherent chunk of a node's `content`. Fused_text = context +
-- "\n\n" + chunk_text is indexed for BM25; `embedding` is the dense vector
-- over the same fused text. `search_context`'s hybrid retrieval fans out
-- over these chunks and groups results back to their owning nodes.
--
-- Chunks are regenerated whole on every content write — the chunk_worker
-- deletes all rows for a `(project_id, node_kind, node_id)` tuple before
-- writing the new batch, so there is no cross-generation drift.
DEFINE TABLE IF NOT EXISTS kg_node_chunk SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id       ON TABLE kg_node_chunk TYPE int;
DEFINE FIELD IF NOT EXISTS node_kind        ON TABLE kg_node_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS node_id          ON TABLE kg_node_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS chunk_index      ON TABLE kg_node_chunk TYPE int;
DEFINE FIELD IF NOT EXISTS chunk_path       ON TABLE kg_node_chunk TYPE array<string> DEFAULT [];
DEFINE FIELD IF NOT EXISTS chunk_text       ON TABLE kg_node_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS context_text     ON TABLE kg_node_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS fused_text       ON TABLE kg_node_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS embedding        ON TABLE kg_node_chunk TYPE array<float>;
DEFINE FIELD IF NOT EXISTS token_count      ON TABLE kg_node_chunk TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS embedding_model  ON TABLE kg_node_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS chunker_version  ON TABLE kg_node_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at       ON TABLE kg_node_chunk TYPE datetime VALUE time::now();

DEFINE INDEX IF NOT EXISTS kg_node_chunk_node_idx
  ON TABLE kg_node_chunk FIELDS project_id, node_kind, node_id, chunk_index UNIQUE;
DEFINE INDEX IF NOT EXISTS kg_node_chunk_search_idx
  ON TABLE kg_node_chunk COLUMNS fused_text FULLTEXT ANALYZER kg_text_analyzer BM25;
DEFINE INDEX IF NOT EXISTS kg_node_chunk_vec_idx
  ON TABLE kg_node_chunk FIELDS embedding HNSW DIMENSION 1024 TYPE F32 DIST COSINE;
