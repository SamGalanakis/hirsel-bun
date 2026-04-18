-- Canvas document (ephemeral, project-scoped, not part of the knowledge graph)
DEFINE TABLE IF NOT EXISTS project_canvas SCHEMALESS;
DEFINE FIELD OVERWRITE updated_at ON TABLE project_canvas VALUE time::now();

-- Canvas spatial layout (per-project map of kind:id -> {x, y})
DEFINE TABLE IF NOT EXISTS project_canvas_layout SCHEMALESS;
DEFINE FIELD OVERWRITE updated_at ON TABLE project_canvas_layout VALUE time::now();
