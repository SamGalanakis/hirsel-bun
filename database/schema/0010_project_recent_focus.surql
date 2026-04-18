-- Tracks the most recently focused canvas/graph nodes per project.
-- Record ID is [project_id, node_kind, node_id] so upsert dedupes by focus target.
DEFINE TABLE IF NOT EXISTS project_recent_focus SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE project_recent_focus TYPE int;
DEFINE FIELD IF NOT EXISTS node_kind ON TABLE project_recent_focus TYPE string;
DEFINE FIELD IF NOT EXISTS node_id ON TABLE project_recent_focus TYPE string;
DEFINE FIELD IF NOT EXISTS focused_at ON TABLE project_recent_focus TYPE datetime VALUE time::now();
DEFINE INDEX IF NOT EXISTS project_recent_focus_recency_idx ON TABLE project_recent_focus FIELDS project_id, focused_at;
