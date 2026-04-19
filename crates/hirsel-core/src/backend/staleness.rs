//! Staleness classification for knowledge-graph nodes.
//!
//! Derived over existing signals:
//!   - `kg_node.updated_at` — when the node's content last changed.
//!   - `kg_node.read_by_search_context` — most recent search surface.
//!   - `kg_read` — per-thread fine-grained read log (Phase 2).
//!
//! Tiers are *derived at read time* — never stored on the node — so they
//! always reflect current activity. The librarian lint sweep translates
//! tiers into tags (e.g. `needs_review`) as an actionable flag; this
//! module only reports.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, DbClient};
use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

/// Coarse tier surfaced in the UI and on `graph.staleness`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StalenessTier {
    /// Recently updated. No concerns.
    Fresh,
    /// Old but actively read in a healthy way (not a pileup).
    Stable,
    /// Old and unread recently. Low priority.
    Stale,
    /// Old AND heavily read in the hot window. Highest review priority —
    /// people depend on it, nobody has re-verified.
    HotAging,
    /// Old and never surfaced at all. Candidate for retirement.
    Unread,
}

/// Thresholds resolved from runtime settings.
#[derive(Debug, Clone, Copy)]
pub struct StalenessConfig {
    pub stale_days: i64,
    pub hot_window_days: i64,
    pub hot_min_reads: i64,
}

impl StalenessConfig {
    pub async fn resolve() -> Self {
        Self {
            stale_days: RuntimeSettings::get_or(
                keys::STALENESS_STALE_DAYS,
                Defaults::STALENESS_STALE_DAYS,
            )
            .await,
            hot_window_days: RuntimeSettings::get_or(
                keys::STALENESS_HOT_WINDOW_DAYS,
                Defaults::STALENESS_HOT_WINDOW_DAYS,
            )
            .await,
            hot_min_reads: RuntimeSettings::get_or(
                keys::STALENESS_HOT_MIN_READS,
                Defaults::STALENESS_HOT_MIN_READS,
            )
            .await,
        }
    }
}

/// Pure classifier: given a node's timestamps + recent-read count,
/// return the tier. Settings-free so tests can exercise without DB.
pub fn classify(
    now: DateTime<Utc>,
    updated_at: Option<DateTime<Utc>>,
    read_by_search_context: Option<DateTime<Utc>>,
    reads_in_hot_window: i64,
    cfg: &StalenessConfig,
) -> StalenessTier {
    let Some(updated_at) = updated_at else {
        return StalenessTier::Unread;
    };

    let age_days = (now - updated_at).num_days();
    if age_days < cfg.stale_days {
        return StalenessTier::Fresh;
    }

    // Old. Classify by read activity.
    if reads_in_hot_window >= cfg.hot_min_reads {
        return StalenessTier::HotAging;
    }

    let has_any_search_read = read_by_search_context.is_some();
    let has_recent_read = reads_in_hot_window > 0 || has_any_search_read;
    if !has_recent_read {
        return StalenessTier::Unread;
    }

    // Old, some reads, but not hot.
    if reads_in_hot_window > 0 {
        StalenessTier::Stable
    } else {
        StalenessTier::Stale
    }
}

/// Structured payload surfaced by the `graph.staleness` tool.
#[derive(Debug, Clone, Serialize)]
pub struct StalenessReport {
    pub kind: String,
    pub node_id: String,
    pub tier: StalenessTier,
    pub age_days: i64,
    pub reads_in_hot_window: i64,
    pub last_read_by_search_context: Option<String>,
    pub updated_at: Option<String>,
}

/// Live classification for a single node. Hits the DB for its
/// timestamps + the `kg_read` aggregate.
pub async fn classify_live(
    project_id: i64,
    kind: &str,
    node_id: &str,
) -> Result<StalenessReport, String> {
    let cfg = StalenessConfig::resolve().await;
    let db = global_db().await;
    let now = Utc::now();

    let (updated_at, read_by_search_context) =
        fetch_node_timestamps(db, project_id, kind, node_id).await?;
    let reads = count_reads_in_window(db, project_id, kind, node_id, cfg.hot_window_days).await;

    let tier = classify(now, updated_at, read_by_search_context, reads, &cfg);
    let age_days = updated_at
        .map(|ts| (now - ts).num_days())
        .unwrap_or(i64::MAX);

    Ok(StalenessReport {
        kind: kind.to_string(),
        node_id: node_id.to_string(),
        tier,
        age_days,
        reads_in_hot_window: reads,
        last_read_by_search_context: read_by_search_context.map(|t| t.to_rfc3339()),
        updated_at: updated_at.map(|t| t.to_rfc3339()),
    })
}

/// Batch variant for populating a list of nodes (e.g. the KG view) in
/// one pass. Returns a map keyed by "kind:node_id".
pub async fn classify_many(
    project_id: i64,
    pairs: &[(String, String)],
) -> std::collections::HashMap<String, StalenessTier> {
    let mut out = std::collections::HashMap::with_capacity(pairs.len());
    for (kind, node_id) in pairs {
        if let Ok(report) = classify_live(project_id, kind, node_id).await {
            out.insert(format!("{}:{}", kind, node_id), report.tier);
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────
// internals
// ─────────────────────────────────────────────────────────────────────

async fn fetch_node_timestamps(
    db: &DbClient,
    project_id: i64,
    kind: &str,
    node_id: &str,
) -> Result<(Option<DateTime<Utc>>, Option<DateTime<Utc>>), String> {
    #[derive(Deserialize, SurrealValue)]
    struct Row {
        #[serde(default)]
        updated_at: Option<DateTime<Utc>>,
        #[serde(default)]
        read_by_search_context: Option<DateTime<Utc>>,
    }
    let mut response = db
        .query(
            "SELECT updated_at, read_by_search_context \
             FROM type::record('kg_node', [$pid, $kind, $nid])",
        )
        .bind(("pid", project_id))
        .bind(("kind", kind.to_string()))
        .bind(("nid", node_id.to_string()))
        .await
        .map_err(|e| format!("fetch_node_timestamps: {e}"))?;
    let rows: Vec<Row> = response.take(0).unwrap_or_default();
    let Some(row) = rows.into_iter().next() else {
        return Ok((None, None));
    };
    Ok((row.updated_at, row.read_by_search_context))
}

async fn count_reads_in_window(
    db: &DbClient,
    project_id: i64,
    kind: &str,
    node_id: &str,
    window_days: i64,
) -> i64 {
    let cutoff = Utc::now() - chrono::Duration::days(window_days);
    let cutoff_str = cutoff.to_rfc3339();
    let query = "SELECT count() AS c FROM kg_read \
                 WHERE project_id = $pid AND node_kind = $kind AND node_id = $nid \
                   AND read_at > type::datetime($cutoff) \
                 GROUP ALL";
    let mut response = match db
        .query(query)
        .bind(("pid", project_id))
        .bind(("kind", kind.to_string()))
        .bind(("nid", node_id.to_string()))
        .bind(("cutoff", cutoff_str))
        .await
    {
        Ok(r) => r,
        Err(_) => return 0,
    };
    #[derive(Deserialize, SurrealValue)]
    struct Row {
        c: i64,
    }
    let rows: Vec<Row> = response.take(0).unwrap_or_default();
    rows.into_iter().next().map(|r| r.c).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> StalenessConfig {
        StalenessConfig {
            stale_days: 30,
            hot_window_days: 7,
            hot_min_reads: 3,
        }
    }

    fn days_ago(n: i64) -> DateTime<Utc> {
        Utc::now() - chrono::Duration::days(n)
    }

    #[test]
    fn fresh_if_recently_updated() {
        let t = classify(Utc::now(), Some(days_ago(5)), None, 0, &cfg());
        assert_eq!(t, StalenessTier::Fresh);
    }

    #[test]
    fn unread_if_old_and_no_reads() {
        let t = classify(Utc::now(), Some(days_ago(60)), None, 0, &cfg());
        assert_eq!(t, StalenessTier::Unread);
    }

    #[test]
    fn hot_aging_if_old_and_many_reads() {
        let t = classify(Utc::now(), Some(days_ago(90)), Some(days_ago(1)), 5, &cfg());
        assert_eq!(t, StalenessTier::HotAging);
    }

    #[test]
    fn stable_if_old_but_quietly_used() {
        let t = classify(Utc::now(), Some(days_ago(60)), Some(days_ago(3)), 1, &cfg());
        assert_eq!(t, StalenessTier::Stable);
    }

    #[test]
    fn no_updated_at_becomes_unread() {
        let t = classify(Utc::now(), None, None, 0, &cfg());
        assert_eq!(t, StalenessTier::Unread);
    }
}
