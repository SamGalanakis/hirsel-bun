use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityProfile {
    Channel,
    Branch,
    CodeWorker,
    OpsWorker,
}

impl CapabilityProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Branch => "branch",
            Self::CodeWorker => "code_worker",
            Self::OpsWorker => "ops_worker",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "channel" => Some(Self::Channel),
            "branch" => Some(Self::Branch),
            "code_worker" => Some(Self::CodeWorker),
            "ops_worker" => Some(Self::OpsWorker),
            _ => None,
        }
    }
}
