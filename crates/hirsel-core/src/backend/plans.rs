use serde::{Deserialize, Serialize};

use crate::backend::shepherd_runtime::ShepherdMessageChunk;
use crate::backend::ShepherdChatMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlanStep {
    pub step: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlanSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    pub plan: Vec<PlanStep>,
}

pub(crate) fn extract_latest_plan(messages: &[ShepherdChatMessage]) -> Option<PlanSnapshot> {
    for message in messages.iter().rev() {
        let Ok(chunks) = serde_json::from_str::<Vec<ShepherdMessageChunk>>(&message.chunks_json)
        else {
            continue;
        };
        for chunk in chunks.into_iter().rev() {
            let ShepherdMessageChunk::Tool { input, .. } = chunk else {
                continue;
            };
            let Some(input) = input else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<PlanSnapshot>(&input) else {
                continue;
            };
            return Some(parsed);
        }
    }
    None
}
