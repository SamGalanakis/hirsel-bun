//! Database schema for board tables

use sqlx::SqlitePool;

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
    assigned_agent_kind TEXT,
    assigned_agent_id TEXT,
    capability_profile TEXT,
    archived_at TEXT,
    PRIMARY KEY (id, project_id, route_id)
);

CREATE INDEX IF NOT EXISTS idx_board_nodes_project ON board_nodes(project_id);
CREATE INDEX IF NOT EXISTS idx_board_nodes_route ON board_nodes(route_id);
CREATE INDEX IF NOT EXISTS idx_board_nodes_parent ON board_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_board_nodes_project_status ON board_nodes(project_id, status);
CREATE INDEX IF NOT EXISTS idx_board_nodes_route_status ON board_nodes(route_id, status);
CREATE INDEX IF NOT EXISTS idx_board_nodes_claimed ON board_nodes(project_id, claimed_by) WHERE status = 'working';
CREATE INDEX IF NOT EXISTS idx_board_nodes_assigned_agent ON board_nodes(project_id, route_id, assigned_agent_kind, assigned_agent_id);
CREATE INDEX IF NOT EXISTS idx_board_nodes_archived ON board_nodes(project_id, route_id, archived_at);

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

CREATE TABLE IF NOT EXISTS route_runtimes (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    runtime_name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'paused',
    created_at TEXT NOT NULL,
    last_dispatch_at TEXT,
    UNIQUE(project_id, route_id)
);

CREATE INDEX IF NOT EXISTS idx_route_runtimes_project ON route_runtimes(project_id);
CREATE INDEX IF NOT EXISTS idx_route_runtimes_route ON route_runtimes(route_id);

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

CREATE TABLE IF NOT EXISTS work_item_events (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    route_id INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    agent_kind TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    details TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_work_item_events_item ON work_item_events(project_id, route_id, item_id, created_at DESC);
"#;

pub async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(SCHEMA).execute(pool).await?;
    Ok(())
}
