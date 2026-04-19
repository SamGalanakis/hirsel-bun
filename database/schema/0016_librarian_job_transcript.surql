-- Additions to `librarian_job` for the UI inspector:
-- - `response_chunks_json` captures the agent's reply as a JSON array of
--   ShepherdMessageChunk objects so the frontend can re-render the trace
--   through the normal chat components.
-- - `reasons_json` holds the structured trigger list (one entry per T1–T5
--   / A1–A4 fire) so the UI can surface them as chips without re-parsing
--   the prompt text.
DEFINE FIELD IF NOT EXISTS response_chunks_json ON TABLE librarian_job TYPE option<string>;
DEFINE FIELD IF NOT EXISTS reasons_json ON TABLE librarian_job TYPE option<string>;
