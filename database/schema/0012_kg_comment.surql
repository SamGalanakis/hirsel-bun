-- Additive, targetable comments attached to knowledge-graph nodes.
-- Each `graph.comment` call inserts a new row; never stomps. The `target`
-- object is optional: when None the comment is about the whole node,
-- otherwise it points at a property and/or line span within a text field.
DEFINE TABLE IF NOT EXISTS kg_comment SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS comment_id  ON TABLE kg_comment TYPE string;
DEFINE FIELD IF NOT EXISTS project_id  ON TABLE kg_comment TYPE int;
DEFINE FIELD IF NOT EXISTS node_kind   ON TABLE kg_comment TYPE string;
DEFINE FIELD IF NOT EXISTS node_id     ON TABLE kg_comment TYPE string;
-- target payload: JSON-encoded `{property?, line_start?, line_end?}`.
-- NONE for whole-node comments.
DEFINE FIELD IF NOT EXISTS target_json ON TABLE kg_comment TYPE option<string>;
DEFINE FIELD IF NOT EXISTS body        ON TABLE kg_comment TYPE string;
DEFINE FIELD IF NOT EXISTS author      ON TABLE kg_comment TYPE string;
DEFINE FIELD IF NOT EXISTS posted_at   ON TABLE kg_comment TYPE string;
DEFINE FIELD IF NOT EXISTS resolved_at ON TABLE kg_comment TYPE option<string>;
DEFINE INDEX IF NOT EXISTS kg_comment_node_idx ON TABLE kg_comment FIELDS project_id, node_kind, node_id, posted_at;
DEFINE INDEX IF NOT EXISTS kg_comment_author_idx ON TABLE kg_comment FIELDS author, posted_at;
