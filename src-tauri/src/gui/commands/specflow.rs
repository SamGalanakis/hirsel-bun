//! SpecFlow board commands
//!
//! Commands for managing the SpecFlow board: tasks, evals, and bookmarks.
//! Also includes Gyp integration for AI-assisted board editing.

use std::sync::Arc;

use futures::StreamExt;

use crate::core::board::{
    BoardService, Bookmark, CreateEvalRequest, CreateTaskRequest, Eval, EvalStatus, SyncResult,
    Task, TaskStatus, TaskTree, UpdateEvalRequest, UpdateTaskRequest,
};
use crate::core::{BoardGypContext, ChatContext, GypChatMessage, GypChatStore, ProjectStore};
use crate::gui::commands::chat::ChatOrchestratorManager;

// ========== TASK COMMANDS ==========

/// Get all tasks for a project as a flat list
#[tauri::command]
pub async fn get_board_tasks(project_id: i64) -> Result<Vec<Task>, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = BoardService::new(project_id);
    service.get_tasks().map_err(|e| e.to_string())
}

/// Get the task tree for a project (with validation computed)
#[tauri::command]
pub async fn get_board_task_tree(project_id: i64) -> Result<Vec<TaskTree>, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = BoardService::new(project_id);
    service.get_task_tree().map_err(|e| e.to_string())
}

/// Create a new task
#[tauri::command]
pub async fn create_board_task(
    project_id: i64,
    parent_id: Option<String>,
    name: String,
    content: Option<String>,
) -> Result<Task, String> {
    let service = BoardService::new(project_id);
    service
        .create_task(&CreateTaskRequest {
            parent_id,
            name,
            content: content.unwrap_or_default(),
        })
        .map_err(|e| e.to_string())
}

/// Update a task
#[tauri::command]
pub async fn update_board_task(
    project_id: i64,
    task_id: String,
    name: Option<String>,
    status: Option<String>,
    content: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Task, String> {
    let service = BoardService::new(project_id);
    service
        .update_task(
            &task_id,
            &UpdateTaskRequest {
                name,
                status: status.map(|s| TaskStatus::from_str(&s)),
                content,
                x,
                y,
            },
        )
        .map_err(|e| e.to_string())
}

/// Delete a task (and all descendants)
#[tauri::command]
pub async fn delete_board_task(project_id: i64, task_id: String) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service.delete_task(&task_id).map_err(|e| e.to_string())
}

/// Move a task to a new parent and/or position
#[tauri::command]
pub async fn move_board_task(
    project_id: i64,
    task_id: String,
    new_parent_id: Option<String>,
    new_position: i32,
) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service
        .move_task(&task_id, new_parent_id.as_deref(), new_position)
        .map_err(|e| e.to_string())
}

// ========== EVAL COMMANDS ==========

/// Get all evals for a project
#[tauri::command]
pub async fn get_board_evals(project_id: i64) -> Result<Vec<Eval>, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = BoardService::new(project_id);
    service.get_evals().map_err(|e| e.to_string())
}

/// Create a new eval
#[tauri::command]
pub async fn create_board_eval(
    project_id: i64,
    name: String,
    content: Option<String>,
    validates: Option<Vec<String>>,
) -> Result<Eval, String> {
    let service = BoardService::new(project_id);
    service
        .create_eval(&CreateEvalRequest {
            name,
            content: content.unwrap_or_default(),
            validates: validates.unwrap_or_default(),
        })
        .map_err(|e| e.to_string())
}

/// Update an eval
#[tauri::command]
pub async fn update_board_eval(
    project_id: i64,
    eval_id: String,
    name: Option<String>,
    status: Option<String>,
    content: Option<String>,
    validates: Option<Vec<String>>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Eval, String> {
    let service = BoardService::new(project_id);
    service
        .update_eval(
            &eval_id,
            &UpdateEvalRequest {
                name,
                status: status.map(|s| EvalStatus::from_str(&s)),
                content,
                validates,
                x,
                y,
            },
        )
        .map_err(|e| e.to_string())
}

/// Delete an eval
#[tauri::command]
pub async fn delete_board_eval(project_id: i64, eval_id: String) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service.delete_eval(&eval_id).map_err(|e| e.to_string())
}

// ========== BOOKMARK COMMANDS ==========

/// Get all bookmarks for a project
#[tauri::command]
pub async fn get_bookmarks(project_id: i64) -> Result<Vec<Bookmark>, String> {
    let service = BoardService::new(project_id);
    service.list_bookmarks().map_err(|e| e.to_string())
}

/// Save a bookmark
#[tauri::command]
pub async fn save_bookmark(
    project_id: i64,
    name: String,
    x: f64,
    y: f64,
    zoom: f64,
) -> Result<Bookmark, String> {
    let service = BoardService::new(project_id);
    service
        .save_bookmark(&name, x, y, zoom)
        .map_err(|e| e.to_string())
}

/// Delete a bookmark
#[tauri::command]
pub async fn delete_bookmark(project_id: i64, bookmark_id: String) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service
        .delete_bookmark(&bookmark_id)
        .map_err(|e| e.to_string())
}

// ========== BOARD SYNC COMMANDS ==========

/// Export board to agent JSON file
///
/// Creates/updates board.json at `~/.hirsel/projects/{project_id}/board/board.json`
/// Returns the path to the board directory.
#[tauri::command]
pub async fn export_board_for_agent(project_id: i64) -> Result<String, String> {
    let mut service = BoardService::new(project_id);
    let board_dir = service
        .export_for_agent()
        .await
        .map_err(|e| e.to_string())?;
    Ok(board_dir.to_string_lossy().to_string())
}

/// Import board from agent JSON file
///
/// Reads board.json from the board directory and syncs to the database.
/// Returns a summary of changes made.
#[tauri::command]
pub async fn import_board_from_agent(project_id: i64) -> Result<SyncResult, String> {
    let mut service = BoardService::new(project_id);
    service.import_from_agent().await.map_err(|e| e.to_string())
}

/// Get the board directory path for a project
#[tauri::command]
pub async fn get_board_directory(project_id: i64) -> Result<String, String> {
    let service = BoardService::new(project_id);
    let board_dir = service.board_dir();
    Ok(board_dir.to_string_lossy().to_string())
}

/// Poll for board file changes and import if changed
///
/// Returns a SyncResult indicating what changed.
#[tauri::command]
pub async fn poll_board_changes(project_id: i64) -> Result<SyncResult, String> {
    let mut service = BoardService::new(project_id);
    service.sync_file_changes().await.map_err(|e| e.to_string())
}

// ========== GYP BOARD CHAT COMMANDS ==========

/// Start a board chat session with Gyp
///
/// Returns the session ID. Events will be emitted via Tauri events.
#[tauri::command]
pub async fn start_board_chat_session(
    app: tauri::AppHandle,
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    project_id: i64,
) -> Result<String, String> {
    use crate::core::ChatEvent;
    use tauri::Emitter;

    // Get project info for working directory
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _project = store.get_project(project_id).map_err(|e| e.to_string())?;

    // Build the board-specific system prompt
    let gyp_context = BoardGypContext::new(project_id);
    let system_prompt = gyp_context.build_system_prompt();

    // Export board to files so Gyp can read them (establishes baseline)
    let mut service = BoardService::new(project_id);
    let board_dir = service
        .export_for_agent()
        .await
        .map_err(|e| format!("Failed to export board: {}", e))?;

    let context = ChatContext {
        // Use hirsel __acp-bridge which acts as ACP server and bridges to Claude CLI
        agent_command: vec!["hirsel".to_string(), "__acp-bridge".to_string()],
        working_dir: Some(board_dir.to_string_lossy().to_string()),
        run_name: None,
        system_prompt: Some(system_prompt),
        credentials: None,
    };

    // Get the local orchestrator (board chat is always local)
    let orchestrator = orchestrator_manager.get_orchestrator(None).await?;

    // Start the session
    let session_info = orchestrator
        .start_session(context)
        .await
        .map_err(|e| format!("Failed to start board chat session: {}", e))?;

    let session_id = session_info.session_id.clone();

    // Subscribe to events and forward to frontend
    let mut event_stream = orchestrator
        .subscribe_events(&session_id)
        .await
        .map_err(|e| format!("Failed to subscribe to events: {}", e))?;

    let session_id_clone = session_id.clone();
    let app_clone = app.clone();
    tracing::info!(
        "[board-chat] Starting event forwarder for session {}",
        session_id
    );

    tokio::spawn(async move {
        while let Some(event) = event_stream.next().await {
            // Emit event to frontend with board-specific event name
            if let Err(e) = app_clone.emit("board-chat-event", &event) {
                tracing::error!("[board-chat] Emit error: {:?}", e);
            }

            // Check if session ended
            if matches!(event, ChatEvent::SessionEnded { .. }) {
                break;
            }
        }
        tracing::info!(
            "[board-chat] Event forwarder stopped for {}",
            session_id_clone
        );
    });

    Ok(session_id)
}

/// Send a message in a board chat session
///
/// The message will include invocation context if focus_task_id/name are provided.
#[tauri::command]
pub async fn send_board_chat_message(
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    project_id: i64,
    session_id: String,
    content: String,
    focus_task_id: Option<String>,
    focus_task_name: Option<String>,
) -> Result<(), String> {
    tracing::info!(
        "[board-chat] Sending message to session {}: {}",
        session_id,
        &content[..content.len().min(50)]
    );
    // Build invocation context if we have task focus
    let gyp_context = BoardGypContext::new(project_id);
    let message = match (focus_task_id, focus_task_name) {
        (Some(id), Some(name)) => gyp_context.build_invocation_context(&id, &name, &content),
        _ => gyp_context.build_general_context(&content),
    };

    // Save user message to history
    let store = GypChatStore::open().map_err(|e| e.to_string())?;
    store
        .save_board_message(
            project_id,
            "user",
            &serde_json::json!([{"type": "text", "content": content}]).to_string(),
        )
        .map_err(|e| e.to_string())?;

    // Get the orchestrator and send
    let orchestrator = orchestrator_manager.get_orchestrator(None).await?;
    orchestrator
        .send_message(&session_id, &message, None)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))
}

/// Get board chat history for a project
#[tauri::command]
pub async fn get_board_chat_history(
    project_id: i64,
    limit: Option<usize>,
) -> Result<Vec<GypChatMessage>, String> {
    let store = GypChatStore::open().map_err(|e| e.to_string())?;
    store
        .get_board_messages(project_id, limit.unwrap_or(10))
        .map_err(|e| e.to_string())
}

/// Save a board chat message (for assistant responses)
#[tauri::command]
pub async fn save_board_chat_message(
    project_id: i64,
    role: String,
    chunks_json: String,
) -> Result<i64, String> {
    let store = GypChatStore::open().map_err(|e| e.to_string())?;
    store
        .save_board_message(project_id, &role, &chunks_json)
        .map_err(|e| e.to_string())
}

/// Clear board chat history for a project
#[tauri::command]
pub async fn clear_board_chat_history(project_id: i64) -> Result<(), String> {
    let store = GypChatStore::open().map_err(|e| e.to_string())?;
    store
        .clear_board_messages(project_id)
        .map_err(|e| e.to_string())
}
