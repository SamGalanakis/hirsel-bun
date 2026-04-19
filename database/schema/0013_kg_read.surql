-- Auto-appended read events. Every thread fetch/search_context that
-- surfaces a node results in one row here so later threads can query
-- "which threads have seen this" and "hot nodes across threads".
DEFINE TABLE IF NOT EXISTS kg_read SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE kg_read TYPE int;
DEFINE FIELD IF NOT EXISTS node_kind  ON TABLE kg_read TYPE string;
DEFINE FIELD IF NOT EXISTS node_id    ON TABLE kg_read TYPE string;
DEFINE FIELD IF NOT EXISTS thread_id  ON TABLE kg_read TYPE option<string>;
DEFINE FIELD IF NOT EXISTS turn       ON TABLE kg_read TYPE option<int>;
DEFINE FIELD IF NOT EXISTS read_at    ON TABLE kg_read TYPE datetime VALUE time::now();
DEFINE INDEX IF NOT EXISTS kg_read_node_idx   ON TABLE kg_read FIELDS project_id, node_kind, node_id, read_at;
DEFINE INDEX IF NOT EXISTS kg_read_thread_idx ON TABLE kg_read FIELDS thread_id, read_at;
