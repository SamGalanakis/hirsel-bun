-- Generic key-value runtime settings. One row per setting; key IS the record id.
-- Values are JSON-encoded so any shape fits without schema migration.
-- Tool signatures read the default from here and accept an optional override.
DEFINE TABLE IF NOT EXISTS runtime_setting SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key ON TABLE runtime_setting TYPE string;
DEFINE FIELD IF NOT EXISTS value_json ON TABLE runtime_setting TYPE string;
DEFINE FIELD OVERWRITE updated_at ON TABLE runtime_setting VALUE time::now();
