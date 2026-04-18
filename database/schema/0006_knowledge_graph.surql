-- Librarian agent identity table (record users for project-scoped access)
DEFINE TABLE IF NOT EXISTS agent SCHEMALESS;

-- Access method for librarian record users
DEFINE ACCESS IF NOT EXISTS librarian ON DATABASE TYPE RECORD
  SIGNUP (CREATE agent SET name = $name, project_id = $project_id)
  SIGNIN (SELECT * FROM agent WHERE name = $name AND project_id = $project_id)
  DURATION FOR TOKEN 24h, FOR SESSION 24h;

-- Knowledge graph node table with librarian permissions
DEFINE TABLE OVERWRITE kg_node SCHEMALESS
  PERMISSIONS
    FOR select WHERE id[0] = $auth.project_id
    FOR create FULL
    FOR update WHERE id[0] = $auth.project_id
    FOR delete WHERE id[0] = $auth.project_id;
DEFINE FIELD OVERWRITE updated_at ON TABLE kg_node VALUE time::now();
DEFINE FIELD IF NOT EXISTS tags ON TABLE kg_node TYPE option<array<string>> DEFAULT [];
DEFINE FIELD IF NOT EXISTS read_by_search_context ON TABLE kg_node TYPE option<datetime>;
-- Document nodes (kind = 'document') use subtype to pick render mode: 'markdown' | 'html'.
DEFINE FIELD IF NOT EXISTS subtype ON TABLE kg_node TYPE option<string>;

DEFINE ANALYZER IF NOT EXISTS kg_text_analyzer TOKENIZERS blank, class, punct FILTERS snowball(english), lowercase;
DEFINE INDEX IF NOT EXISTS kg_node_label_search ON TABLE kg_node COLUMNS label FULLTEXT ANALYZER kg_text_analyzer BM25;
DEFINE INDEX IF NOT EXISTS kg_node_content_search ON TABLE kg_node COLUMNS content FULLTEXT ANALYZER kg_text_analyzer BM25;

-- Knowledge graph edge table with librarian permissions
DEFINE TABLE OVERWRITE kg_edge TYPE RELATION SCHEMALESS
  PERMISSIONS
    FOR select WHERE `in`[0] = $auth.project_id AND out[0] = $auth.project_id
    FOR create FULL
    FOR update WHERE `in`[0] = $auth.project_id AND out[0] = $auth.project_id
    FOR delete WHERE `in`[0] = $auth.project_id AND out[0] = $auth.project_id;
DEFINE FIELD OVERWRITE created_at ON TABLE kg_edge DEFAULT time::now();
