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
DEFINE FIELD IF NOT EXISTS search_model ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS search_model_variant ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE app_setting TYPE string;

DEFINE TABLE IF NOT EXISTS credential SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key_type ON TABLE credential TYPE string;
DEFINE FIELD IF NOT EXISTS encrypted_value_b64 ON TABLE credential TYPE string;
DEFINE FIELD IF NOT EXISTS nonce_b64 ON TABLE credential TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE credential TYPE string;
