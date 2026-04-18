use serde::{Deserialize, Serialize};

// ── Scope key ──

pub fn scope_key(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::General => "general".to_string(),
        ShepherdScope::Shepherd { project_id, .. } => format!("shepherd-{project_id}"),
        ShepherdScope::Thread { thread_id, .. } => format!("thread-{thread_id}"),
    }
}

// ── Stream events (sent from worker tasks to command layer) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerStreamEvent {
    TextDelta {
        content: String,
    },
    DurableSnapshot {
        state_json: String,
    },
    Tool {
        id: String,
        title: String,
        #[serde(default)]
        kind: Option<String>,
        status: String,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        output: Option<String>,
    },
    Message {
        text: String,
        kind: String,
    },
    Error {
        message: String,
    },
}

// ── Preview forwarding ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewForwardInfo {
    pub id: String,
    pub project_id: i64,
    pub thread_id: String,
    pub protocol: String,
    pub port: u16,
    pub host_port: u16,
    pub user_url: String,
    pub shepherd_url: String,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyHttpRequest {
    pub method: String,
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
}

// ── Knowledge graph lore ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLoreEntry {
    pub node_id: String,
    pub text: String,
}

// ── Turn result (returned by in-process worker) ──

pub struct TurnResult {
    pub assistant_chunks: Vec<ShepherdMessageChunk>,
    pub state_json: String,
    pub summary: String,
    pub interrupted: bool,
}

// ── Original types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdTaskFocus {
    pub task_id: String,
    pub task_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ShepherdScope {
    #[serde(rename = "general")]
    General,
    #[serde(rename = "shepherd")]
    Shepherd {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
    },
    #[serde(rename = "thread")]
    Thread {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "threadId")]
        thread_id: String,
        title: String,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ShepherdMessageChunk {
    Text {
        content: String,
    },
    Notice {
        tone: String,
        #[serde(default)]
        title: Option<String>,
        content: String,
    },
    Thinking {
        content: String,
    },
    Tool {
        id: String,
        title: String,
        #[serde(default)]
        kind: Option<String>,
        status: String,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        output: Option<String>,
    },
    Image {
        #[serde(rename = "mimeType")]
        mime_type: String,
        #[serde(rename = "dataBase64")]
        data_base64: String,
        #[serde(default)]
        name: Option<String>,
    },
    Skill {
        name: String,
        #[serde(default)]
        description: Option<String>,
        path: String,
    },
    FileRef {
        #[serde(rename = "rootId")]
        root_id: String,
        path: String,
        #[serde(rename = "lineStart", default)]
        line_start: Option<usize>,
        #[serde(rename = "lineEnd", default)]
        line_end: Option<usize>,
    },
}
