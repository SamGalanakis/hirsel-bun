-- Tasks (persistent planning documents with status tracking)
DEFINE TABLE IF NOT EXISTS task SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS task_id ON TABLE task TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE task TYPE int;
DEFINE FIELD IF NOT EXISTS title ON TABLE task TYPE string;
DEFINE FIELD IF NOT EXISTS status ON TABLE task TYPE string;
DEFINE FIELD IF NOT EXISTS content ON TABLE task TYPE option<string>;
DEFINE FIELD IF NOT EXISTS review_json ON TABLE task TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sort_order ON TABLE task TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE task TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE task TYPE string;
DEFINE INDEX IF NOT EXISTS task_project_idx ON TABLE task FIELDS project_id;
