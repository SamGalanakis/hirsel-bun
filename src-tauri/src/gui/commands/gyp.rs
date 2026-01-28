//! Unified Gyp session commands
//!
//! Single entry point for all Gyp chat sessions regardless of scope.
//! Handles session lifecycle, scope changes, and message routing.

use std::sync::Arc;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::core::board::{BoardService, ExportScope};
use crate::core::draft::create_workspace_provider;
use crate::core::gyp::{GypContextBuilder, GypScope, TaskFocus};
use crate::core::{ChatContext, GypChatStore, ProjectStore};
use crate::gui::commands::chat::ChatOrchestratorManager;

/// Request to start a Gyp session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StartGypSessionRequest {
    /// General chat - no project context
    #[serde(rename = "general")]
    General,
    /// Run context
    #[serde(rename = "run")]
    Run {
        #[serde(rename = "runName")]
        run_name: String,
    },
    /// Board context - whole board
    #[serde(rename = "board")]
    Board {
        #[serde(rename = "projectId")]
        project_id: i64,
    },
    /// Board context - focused on specific task
    #[serde(rename = "boardFocused")]
    BoardFocused {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "taskId")]
        task_id: String,
        #[serde(rename = "taskName")]
        task_name: String,
    },
}

/// Response from starting a Gyp session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartGypSessionResponse {
    pub session_id: String,
    pub scope: GypScope,
}

/// Start a unified Gyp session
///
/// Creates a new chat session with appropriate context based on the request type.
/// Returns session ID and the resolved scope.
#[tauri::command]
pub async fn start_gyp_session(
    app: tauri::AppHandle,
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    request: StartGypSessionRequest,
) -> Result<StartGypSessionResponse, String> {
    tracing::info!("[gyp] Starting session with request: {:?}", request);

    // Build context based on request type
    let (builder, _board_export) = match &request {
        StartGypSessionRequest::General => (GypContextBuilder::general(), None),

        StartGypSessionRequest::Run { run_name } => {
            let workspace = create_workspace_provider(None);
            (
                GypContextBuilder::for_run(run_name, workspace.as_ref()),
                None,
            )
        }

        StartGypSessionRequest::Board { project_id } => {
            let store = ProjectStore::open().map_err(|e| e.to_string())?;
            let project = store.get_project(*project_id).map_err(|e| e.to_string())?;

            // Export board files for agent access
            let mut service = BoardService::new(*project_id);
            service
                .export_for_agent(&ExportScope::WholeBoard)
                .await
                .map_err(|e| format!("Failed to export board: {}", e))?;

            (
                GypContextBuilder::for_board(*project_id, &project.starting_point),
                Some(*project_id),
            )
        }

        StartGypSessionRequest::BoardFocused {
            project_id,
            task_id,
            task_name,
        } => {
            let store = ProjectStore::open().map_err(|e| e.to_string())?;
            let project = store.get_project(*project_id).map_err(|e| e.to_string())?;

            // Export board files for agent access
            let mut service = BoardService::new(*project_id);
            service
                .export_for_agent(&ExportScope::FocusedTask {
                    task_id: task_id.clone(),
                    task_name: task_name.clone(),
                })
                .await
                .map_err(|e| format!("Failed to export board: {}", e))?;

            (
                GypContextBuilder::for_board_focused(
                    *project_id,
                    &project.starting_point,
                    task_id.clone(),
                    task_name.clone(),
                ),
                Some(*project_id),
            )
        }
    };

    let config = builder.build();
    let scope = builder.scope().clone();

    // Use current executable path for agent command (works in dev and production)
    let exe_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "hirsel".to_string());

    let chat_context = ChatContext {
        agent_command: vec![exe_path, "__acp-bridge".to_string()],
        working_dir: Some(config.working_dir.to_string_lossy().to_string()),
        // run_name triggers MCP configuration in chat_session
        run_name: match &scope {
            GypScope::Run { run_name, .. } => Some(run_name.clone()),
            _ => None,
        },
        system_prompt: Some(config.system_prompt),
        credentials: None,
    };

    // Get orchestrator (always local for Gyp)
    let orchestrator = orchestrator_manager.get_orchestrator(None).await?;

    // Start session
    let session_info = orchestrator
        .start_session(chat_context)
        .await
        .map_err(|e| format!("Failed to start session: {}", e))?;

    let session_id = session_info.session_id.clone();

    // Subscribe to events and forward to frontend
    let mut event_stream = orchestrator
        .subscribe_events(&session_id)
        .await
        .map_err(|e| format!("Failed to subscribe to events: {}", e))?;

    let app_handle = app.clone();
    let sid_clone = session_id.clone();

    tokio::spawn(async move {
        while let Some(event) = event_stream.next().await {
            // Forward to frontend (assistant message saving happens in frontend)
            if let Err(e) = app_handle.emit("gyp-event", (&sid_clone, &event)) {
                tracing::warn!("[gyp] Failed to emit event: {}", e);
            }
        }

        tracing::info!("[gyp] Event stream ended for session {}", sid_clone);
    });

    tracing::info!("[gyp] Session started: {}", session_id);

    Ok(StartGypSessionResponse { session_id, scope })
}

/// Send a message in a Gyp session
///
/// The message may be augmented with context based on the current scope.
/// - `scope`: Current scope for context injection
/// - `focus`: Updated focus (for board context)
#[tauri::command]
pub async fn send_gyp_message(
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    session_id: String,
    content: String,
    scope: GypScope,
    focus: Option<TaskFocus>,
) -> Result<(), String> {
    tracing::info!(
        "[gyp] Sending message to session {}: {}...",
        session_id,
        &content[..content.len().min(50)]
    );

    // Build message with context injection based on scope
    let scope_with_focus = match (scope, focus) {
        (
            GypScope::Board {
                project_id,
                workspace_path,
                ..
            },
            Some(f),
        ) => GypScope::Board {
            project_id,
            workspace_path,
            focus: Some(f),
        },
        (s, _) => s,
    };

    let builder = match &scope_with_focus {
        GypScope::General => GypContextBuilder::general(),
        GypScope::Run { run_name, .. } => {
            let workspace = create_workspace_provider(None);
            GypContextBuilder::for_run(run_name, workspace.as_ref())
        }
        GypScope::Board {
            project_id,
            focus: None,
            ..
        } => {
            let store = ProjectStore::open().map_err(|e| e.to_string())?;
            let project = store.get_project(*project_id).map_err(|e| e.to_string())?;
            GypContextBuilder::for_board(*project_id, &project.starting_point)
        }
        GypScope::Board {
            project_id,
            focus: Some(f),
            ..
        } => {
            let store = ProjectStore::open().map_err(|e| e.to_string())?;
            let project = store.get_project(*project_id).map_err(|e| e.to_string())?;
            GypContextBuilder::for_board_focused(
                *project_id,
                &project.starting_point,
                f.task_id.clone(),
                f.task_name.clone(),
            )
        }
    };

    let message = builder.build_message_context(&content);
    let history_scope = builder.build().history_scope;

    // Save user message to history
    if let Ok(store) = GypChatStore::open() {
        let chunks_json = serde_json::json!([{"type": "text", "content": content}]).to_string();
        let _ = match (&history_scope.project_id, &history_scope.run_name) {
            (Some(pid), None) => store.save_board_message(*pid, "user", &chunks_json),
            (_, Some(run)) => store.save_message(Some(run), "user", &chunks_json),
            _ => store.save_message(None, "user", &chunks_json),
        };
    }

    // Send to orchestrator
    let orchestrator = orchestrator_manager.get_orchestrator(None).await?;
    orchestrator
        .send_message(&session_id, &message, None)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))?;

    Ok(())
}

/// Get Gyp chat history for a scope
#[tauri::command]
pub async fn get_gyp_history(
    scope: GypScope,
    limit: usize,
) -> Result<Vec<crate::core::GypChatMessage>, String> {
    let store = GypChatStore::open().map_err(|e| e.to_string())?;

    let messages = match &scope {
        // Note: get_messages doesn't support limit, returns all messages
        GypScope::General => store.get_messages(None),
        GypScope::Run { run_name, .. } => store.get_messages(Some(run_name)),
        GypScope::Board { project_id, .. } => store.get_board_messages(*project_id, limit),
    }
    .map_err(|e| e.to_string())?;

    // Apply limit for non-board scopes (board already has limit in query)
    let messages = match &scope {
        GypScope::Board { .. } => messages,
        _ => messages.into_iter().take(limit).collect(),
    };

    Ok(messages)
}

/// Clear Gyp chat history for a scope
#[tauri::command]
pub async fn clear_gyp_history(scope: GypScope) -> Result<(), String> {
    let store = GypChatStore::open().map_err(|e| e.to_string())?;

    match scope {
        GypScope::General => store.clear_messages(None),
        GypScope::Run { run_name, .. } => store.clear_messages(Some(&run_name)),
        GypScope::Board { project_id, .. } => store.clear_board_messages(project_id),
    }
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Save a Gyp message to history
///
/// Unified save command that handles all scopes (board, run, general).
#[tauri::command]
pub async fn save_gyp_message(
    scope: GypScope,
    role: String,
    chunks_json: String,
) -> Result<i64, String> {
    let store = GypChatStore::open().map_err(|e| e.to_string())?;

    match &scope {
        GypScope::Board { project_id, .. } => {
            store.save_board_message(*project_id, &role, &chunks_json)
        }
        GypScope::Run { run_name, .. } => store.save_message(Some(run_name), &role, &chunks_json),
        GypScope::General => store.save_message(None, &role, &chunks_json),
    }
    .map_err(|e| e.to_string())
}

/// Stop a Gyp session
#[tauri::command]
pub async fn stop_gyp_session(
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    session_id: String,
) -> Result<(), String> {
    tracing::info!("[gyp] Stopping session: {}", session_id);

    let orchestrator = orchestrator_manager.get_orchestrator(None).await?;
    orchestrator
        .stop_session(&session_id)
        .await
        .map_err(|e| format!("Failed to stop session: {}", e))?;

    Ok(())
}
