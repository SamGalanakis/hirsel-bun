-- Shepherd event queue (batched input for event-driven shepherd turns)
DEFINE TABLE IF NOT EXISTS shepherd_event SCHEMALESS;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE shepherd_event DEFAULT time::now();

-- Librarian event queue (user activity signals: "user created X", "user edited Y", "user deleted Z")
DEFINE TABLE IF NOT EXISTS librarian_event SCHEMALESS;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE librarian_event DEFAULT time::now();

-- Librarian job queue (one-shot agent runs triggered by chat sync and periodic lint).
-- The server pops jobs one at a time, spins up a fresh Lash runtime with
-- graph-patch tools, and discards the runtime when the job finishes.
DEFINE TABLE IF NOT EXISTS librarian_job SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE librarian_job TYPE int;
DEFINE FIELD IF NOT EXISTS kind ON TABLE librarian_job TYPE string;
DEFINE FIELD IF NOT EXISTS prompt ON TABLE librarian_job TYPE string;
DEFINE FIELD IF NOT EXISTS status ON TABLE librarian_job TYPE string DEFAULT 'queued';
DEFINE FIELD IF NOT EXISTS last_error ON TABLE librarian_job TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE librarian_job DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE librarian_job VALUE time::now();
DEFINE INDEX IF NOT EXISTS librarian_job_queue_idx ON TABLE librarian_job FIELDS status, created_at;
