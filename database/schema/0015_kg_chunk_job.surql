-- Chunking + embedding job queue.
--
-- `apply_graph_patch` enqueues a row whenever a node's `content` changes
-- (upsert_node set.content, patch_node_text, supersede_node). The
-- chunk_worker claims the oldest queued row, loads the node, splits
-- content into chunks, contextualises each, batch-embeds via OpenRouter,
-- and upserts into `kg_node_chunk` (delete-then-insert for the node
-- tuple).
DEFINE TABLE IF NOT EXISTS kg_chunk_job SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE kg_chunk_job TYPE int;
DEFINE FIELD IF NOT EXISTS node_kind  ON TABLE kg_chunk_job TYPE string;
DEFINE FIELD IF NOT EXISTS node_id    ON TABLE kg_chunk_job TYPE string;
DEFINE FIELD IF NOT EXISTS status     ON TABLE kg_chunk_job TYPE string DEFAULT 'queued';
DEFINE FIELD IF NOT EXISTS last_error ON TABLE kg_chunk_job TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE kg_chunk_job DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE kg_chunk_job VALUE time::now();
DEFINE INDEX IF NOT EXISTS kg_chunk_job_queue_idx
  ON TABLE kg_chunk_job FIELDS status, created_at;
-- Dedup: if a job for this node is already queued, reuse the row instead
-- of creating a duplicate. Enforced by the enqueue helper; this index
-- makes the lookup cheap.
DEFINE INDEX IF NOT EXISTS kg_chunk_job_node_idx
  ON TABLE kg_chunk_job FIELDS project_id, node_kind, node_id, status;
