use serde::{Deserialize, Serialize};

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
    #[serde(rename = "project")]
    Project {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<String>,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
    },
    #[serde(rename = "branch")]
    Branch {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "branchId")]
        branch_id: String,
        #[serde(rename = "parentSessionId")]
        parent_session_id: String,
        goal: String,
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<String>,
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
}
