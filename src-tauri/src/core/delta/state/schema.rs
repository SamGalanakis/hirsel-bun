//! Database schema for board tables

use sqlx::SqlitePool;
use tokio::sync::OnceCell;

/// Schema for board tables
///
/// All route-scoped tables include route_id for isolation between parallel exploration routes.
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS board_nodes (
    id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    parent_id TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'task',
    source TEXT NOT NULL DEFAULT 'user',
    content TEXT NOT NULL DEFAULT '',
    difficulty TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'draft',
    x REAL,
    y REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    last_commit_sha TEXT,
    resolves TEXT,
    -- Orchestration fields
    claimed_by TEXT,
    claimed_at TEXT,
    completed_by TEXT,
    check_result TEXT,
    check_feedback TEXT,
    tokens_used INTEGER,
    PRIMARY KEY (id, project_id, route_id)
);

CREATE INDEX IF NOT EXISTS idx_board_nodes_project ON board_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_board_nodes_route ON board_nodes(route_id);
CREATE INDEX IF NOT EXISTS idx_board_nodes_parent ON board_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_board_nodes_project_status ON board_nodes(project_id, status);
CREATE INDEX IF NOT EXISTS idx_board_nodes_route_status ON board_nodes(route_id, status);
CREATE INDEX IF NOT EXISTS idx_board_nodes_claimed ON board_nodes(project_id, claimed_by) WHERE status = 'working';

CREATE TABLE IF NOT EXISTS board_node_checked_by (
    check_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    PRIMARY KEY (project_id, route_id, check_id, node_id)
);
CREATE INDEX IF NOT EXISTS idx_board_checked_by_check ON board_node_checked_by(check_id);
CREATE INDEX IF NOT EXISTS idx_board_checked_by_node ON board_node_checked_by(node_id);
CREATE INDEX IF NOT EXISTS idx_board_checked_by_route ON board_node_checked_by(route_id);

CREATE TABLE IF NOT EXISTS board_node_blocked_by (
    node_id TEXT NOT NULL,
    blocker_id TEXT NOT NULL,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    PRIMARY KEY (project_id, route_id, node_id, blocker_id)
);
CREATE INDEX IF NOT EXISTS idx_board_blocked_node ON board_node_blocked_by(node_id);
CREATE INDEX IF NOT EXISTS idx_board_blocked_blocker ON board_node_blocked_by(blocker_id);
CREATE INDEX IF NOT EXISTS idx_board_blocked_route ON board_node_blocked_by(route_id);

CREATE TABLE IF NOT EXISTS project_runs (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    run_name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'paused',
    created_at TEXT NOT NULL,
    last_dispatch_at TEXT,
    UNIQUE(project_id, route_id)
);

CREATE INDEX IF NOT EXISTS idx_project_runs_project ON project_runs(project_id);
CREATE INDEX IF NOT EXISTS idx_project_runs_route ON project_runs(route_id);

CREATE TABLE IF NOT EXISTS board_versions (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    version_number INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    description TEXT,
    UNIQUE(project_id, route_id, version_number)
);

CREATE INDEX IF NOT EXISTS idx_board_versions_project ON board_versions(project_id);
CREATE INDEX IF NOT EXISTS idx_board_versions_route ON board_versions(route_id);

CREATE TABLE IF NOT EXISTS deliveries (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    version_id INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    target_branch TEXT NOT NULL,
    delivery_branch TEXT,
    pr_url TEXT,
    pr_number INTEGER,
    started_at TEXT,
    completed_at TEXT,
    failure_reason TEXT,
    FOREIGN KEY (version_id) REFERENCES board_versions(id)
);

CREATE INDEX IF NOT EXISTS idx_deliveries_project ON deliveries(project_id);
CREATE INDEX IF NOT EXISTS idx_deliveries_route ON deliveries(route_id);
CREATE INDEX IF NOT EXISTS idx_deliveries_version ON deliveries(version_id);
CREATE INDEX IF NOT EXISTS idx_deliveries_status ON deliveries(status);

CREATE TABLE IF NOT EXISTS delivery_attempts (
    id INTEGER PRIMARY KEY,
    delivery_id INTEGER NOT NULL,
    attempt_number INTEGER NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    error_message TEXT,
    FOREIGN KEY (delivery_id) REFERENCES deliveries(id)
);

CREATE INDEX IF NOT EXISTS idx_delivery_attempts_delivery ON delivery_attempts(delivery_id);

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL DEFAULT 0
);
"#;

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

pub async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    SCHEMA_INIT
        .get_or_try_init(|| async {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
            let migration = sqlx::query(
                "ALTER TABLE board_nodes ADD COLUMN difficulty TEXT NOT NULL DEFAULT 'medium'",
            )
            .execute(pool)
            .await;
            if let Err(e) = migration {
                let msg = e.to_string().to_ascii_lowercase();
                if !msg.contains("duplicate column name") && !msg.contains("already exists") {
                    return Err(e);
                }
            }
            Ok::<(), sqlx::Error>(())
        })
        .await?;
    Ok(())
}
