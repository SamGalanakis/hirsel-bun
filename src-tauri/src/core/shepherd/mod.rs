//! Shepherd orchestration domain primitives.
//!
//! This module defines the first-class Shepherd abstraction used to drive
//! orchestration decisions independently from daemon process supervision.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShepherdDecisionType {
    Decompose,
    Assign,
    Replan,
    Validate,
    Deliver,
    Message,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShepherdCommandType {
    SpawnWorker,
    ResumeWorker,
    StopWorker,
    RunCheck,
    CompleteRun,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShepherdCommandStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdDecision {
    pub run_name: String,
    pub decision_type: ShepherdDecisionType,
    pub summary: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdCommand {
    pub run_name: String,
    pub command_type: ShepherdCommandType,
    pub status: ShepherdCommandStatus,
    pub payload_json: String,
    pub created_at: String,
}

#[allow(async_fn_in_trait)]
pub trait ShepherdEngine: Send + Sync {
    async fn start_run_context(&self, project_id: i64, route_id: i64) -> Result<String, String>;

    async fn ingest_user_message(
        &self,
        run_name: &str,
        message: &str,
    ) -> Result<ShepherdDecision, String>;

    async fn issue_assignments(&self, run_name: &str) -> Result<Vec<ShepherdCommand>, String>;
}

/// Minimal local Shepherd engine bootstrap.
///
/// This intentionally starts lightweight while command/event persistence and
/// daemon integration are migrated in.
pub struct LocalShepherdEngine;

#[allow(async_fn_in_trait)]
impl ShepherdEngine for LocalShepherdEngine {
    async fn start_run_context(&self, project_id: i64, route_id: i64) -> Result<String, String> {
        Ok(format!("shepherd:{}:{}", project_id, route_id))
    }

    async fn ingest_user_message(
        &self,
        run_name: &str,
        message: &str,
    ) -> Result<ShepherdDecision, String> {
        Ok(ShepherdDecision {
            run_name: run_name.to_string(),
            decision_type: ShepherdDecisionType::Message,
            summary: "Accepted user instruction".to_string(),
            payload_json: serde_json::json!({ "message": message }).to_string(),
            created_at: crate::core::db::utc_now(),
        })
    }

    async fn issue_assignments(&self, run_name: &str) -> Result<Vec<ShepherdCommand>, String> {
        Ok(vec![ShepherdCommand {
            run_name: run_name.to_string(),
            command_type: ShepherdCommandType::SpawnWorker,
            status: ShepherdCommandStatus::Pending,
            payload_json: "{}".to_string(),
            created_at: crate::core::db::utc_now(),
        }])
    }
}
