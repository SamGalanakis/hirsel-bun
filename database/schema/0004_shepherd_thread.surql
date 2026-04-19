DEFINE TABLE IF NOT EXISTS shepherd_thread SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS thread_id ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_thread TYPE int;
DEFINE FIELD IF NOT EXISTS title ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS title_lower ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS objective ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS summary ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS status ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS workspace_path ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS checkout_name ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS last_activity_at ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS archived_at ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS highlight ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS focused_task_id ON TABLE shepherd_thread TYPE option<string>;

-- Unified-thread columns (phase 1, additive).
-- `parent_id` links spawned children to their parent thread.
-- `binding_kind` + `binding_data` describe what the thread is bound to.
--   kind ∈ {free, task, node, file, workspace}; data is a JSON string
--   with per-kind payload (node_kind+node_id, path, etc.).
-- `capabilities` is the set of tool-access atoms for this thread.
-- `sort_order` orders threads in list/board views.
-- `final_output` is populated when the thread completes; read via await_thread.
-- `merge_status` tracks post-completion workspace merge state.
DEFINE FIELD IF NOT EXISTS parent_id ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS binding_kind ON TABLE shepherd_thread TYPE string DEFAULT 'free';
DEFINE FIELD IF NOT EXISTS binding_data ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS capabilities ON TABLE shepherd_thread TYPE array<string> DEFAULT [];
DEFINE FIELD IF NOT EXISTS sort_order ON TABLE shepherd_thread TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS final_output ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS merge_status ON TABLE shepherd_thread TYPE string DEFAULT 'none';
-- task absorption: `content` carries the planning-doc markdown body,
-- `review_json` carries the review payload for threads that went through a
-- review cycle.
DEFINE FIELD IF NOT EXISTS content ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS review_json ON TABLE shepherd_thread TYPE option<string>;

DEFINE INDEX IF NOT EXISTS shepherd_thread_project_idx ON TABLE shepherd_thread FIELDS project_id;
DEFINE INDEX IF NOT EXISTS shepherd_thread_title_idx ON TABLE shepherd_thread FIELDS project_id, title_lower;
DEFINE INDEX IF NOT EXISTS shepherd_thread_parent_idx ON TABLE shepherd_thread FIELDS parent_id;
