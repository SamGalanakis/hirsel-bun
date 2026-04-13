
DEFINE TABLE IF NOT EXISTS counter SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS value ON TABLE counter TYPE int;

DEFINE TABLE IF NOT EXISTS app_setting SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS provider ON TABLE app_setting TYPE string;
DEFINE FIELD IF NOT EXISTS openrouter_base_url ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS shepherd_model ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS shepherd_model_variant ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS librarian_model ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS librarian_model_variant ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread_model ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread_model_variant ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE app_setting TYPE string;

DEFINE TABLE IF NOT EXISTS project SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE project TYPE int;
DEFINE FIELD IF NOT EXISTS name ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS name_lower ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS icon ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS workspaces ON TABLE project TYPE array<object>;
DEFINE FIELD IF NOT EXISTS workspaces[*].id ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS workspaces[*].kind ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS workspaces[*].label ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS workspaces[*].path ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS workspaces[*].url ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS workspaces[*].branch ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS shepherd_cwd ON TABLE project TYPE option<string>;
DEFINE INDEX IF NOT EXISTS project_name_lower_idx ON TABLE project FIELDS name_lower UNIQUE;

DEFINE TABLE IF NOT EXISTS project_retained_context SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE project_retained_context TYPE int;
DEFINE FIELD IF NOT EXISTS markdown ON TABLE project_retained_context TYPE string;
DEFINE FIELD IF NOT EXISTS source ON TABLE project_retained_context TYPE option<string>;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE project_retained_context TYPE string;

DEFINE TABLE IF NOT EXISTS shepherd_chat_message SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS message_id ON TABLE shepherd_chat_message TYPE int;
DEFINE FIELD IF NOT EXISTS lookup_key ON TABLE shepherd_chat_message TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_chat_message TYPE option<int>;
DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_chat_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS role ON TABLE shepherd_chat_message TYPE string;
DEFINE FIELD IF NOT EXISTS timestamp ON TABLE shepherd_chat_message TYPE string;
DEFINE FIELD IF NOT EXISTS message_kind ON TABLE shepherd_chat_message TYPE string;
DEFINE FIELD IF NOT EXISTS preview_text ON TABLE shepherd_chat_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS collapsed_by_default ON TABLE shepherd_chat_message TYPE bool;
DEFINE FIELD IF NOT EXISTS chunks_json ON TABLE shepherd_chat_message TYPE string;
DEFINE INDEX IF NOT EXISTS shepherd_chat_lookup_idx ON TABLE shepherd_chat_message FIELDS lookup_key;

DEFINE TABLE IF NOT EXISTS shepherd_live_turn SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS lookup_key ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_live_turn TYPE option<int>;
DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS role ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS chunks_json ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS status ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS error ON TABLE shepherd_live_turn TYPE option<string>;
DEFINE FIELD IF NOT EXISTS started_at ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_live_turn TYPE string;

DEFINE TABLE IF NOT EXISTS shepherd_scope_state SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS lookup_key ON TABLE shepherd_scope_state TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_scope_state TYPE int;
DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_scope_state TYPE string;
DEFINE FIELD IF NOT EXISTS state_json ON TABLE shepherd_scope_state TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_scope_state TYPE string;

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
DEFINE INDEX IF NOT EXISTS shepherd_thread_project_idx ON TABLE shepherd_thread FIELDS project_id;
DEFINE INDEX IF NOT EXISTS shepherd_thread_title_idx ON TABLE shepherd_thread FIELDS project_id, title_lower;

    DEFINE TABLE IF NOT EXISTS shepherd_session SCHEMAFULL;
    DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_session TYPE option<int>;
    DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_session TYPE string;
    DEFINE FIELD IF NOT EXISTS scope_json ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS workspace_path ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS env_fingerprint ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS runtime_fingerprint ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS status ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS container_name ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS socket_path ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS last_error ON TABLE shepherd_session TYPE option<string>;
    DEFINE FIELD IF NOT EXISTS created_at ON TABLE shepherd_session TYPE string;
    DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_session TYPE string;
    DEFINE FIELD IF NOT EXISTS last_seen_at ON TABLE shepherd_session TYPE option<string>;

    DEFINE TABLE IF NOT EXISTS credential SCHEMAFULL;
    DEFINE FIELD IF NOT EXISTS key_type ON TABLE credential TYPE string;
    DEFINE FIELD IF NOT EXISTS encrypted_value_b64 ON TABLE credential TYPE string;
    DEFINE FIELD IF NOT EXISTS nonce_b64 ON TABLE credential TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE credential TYPE string;

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

-- Canvas document (ephemeral, project-scoped, not part of the knowledge graph)
DEFINE TABLE IF NOT EXISTS project_canvas SCHEMALESS;
DEFINE FIELD OVERWRITE updated_at ON TABLE project_canvas VALUE time::now();

-- Shepherd event queue (batched input for event-driven shepherd turns)
DEFINE TABLE IF NOT EXISTS shepherd_event SCHEMALESS;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE shepherd_event DEFAULT time::now();
