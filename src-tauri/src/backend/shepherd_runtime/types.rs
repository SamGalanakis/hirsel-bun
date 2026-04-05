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
    #[serde(rename = "shepherd")]
    Shepherd {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<String>,
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
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<String>,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
    },
    #[serde(rename = "librarian")]
    Librarian {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<String>,
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
