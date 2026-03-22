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
    #[serde(rename = "run")]
    Run {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workspacePath", default)]
        workspace_path: String,
        #[serde(rename = "projectPath", default)]
        project_path: Option<String>,
    },
    #[serde(rename = "project")]
    Project {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<String>,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StartShepherdSessionRequest {
    #[serde(rename = "general")]
    General,
    #[serde(rename = "run")]
    Run {
        #[serde(rename = "runName")]
        run_name: String,
    },
    #[serde(rename = "project")]
    Project {
        #[serde(rename = "projectId")]
        project_id: i64,
    },
    #[serde(rename = "projectFocused")]
    ProjectFocused {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "taskId")]
        task_id: String,
        #[serde(rename = "taskName")]
        task_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartShepherdSessionResponse {
    pub session_id: String,
    pub scope: ShepherdScope,
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
