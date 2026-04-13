use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{PluginSpec, PromptContribution, ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Value};

use crate::backend::tasks::TaskStore;
use crate::backend::text_patch::apply_text_patch;
use crate::backend::tool_results::edit_result_with;
use crate::backend::ShepherdThreadStore;

const TEXT_PATCH_INSTRUCTIONS: &str = crate::backend::text_patch::TEXT_PATCH_INSTRUCTIONS;

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

struct TaskToolProvider {
    project_id: i64,
    thread_id: Option<String>,
}

#[async_trait::async_trait]
impl ToolProvider for TaskToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut defs = vec![
            tool_definition! {
                name: "list_tasks".to_string(),
                description: "List all tasks for the current project with their status and content preview.".to_string(),
                params: vec![],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "create_task".to_string(),
                description: "Create a new task. Tasks are persistent planning documents with title, status, and markdown content.".to_string(),
                params: vec![
                    ToolParam::typed("title", "str"),
                    ToolParam::optional("content", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "update_task".to_string(),
                description: "Update a task's title, status (todo/active/done), or content.".to_string(),
                params: vec![
                    ToolParam::typed("task_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::optional("status", "str"),
                    ToolParam::optional("content", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ];

        // Thread-only tools (need thread_id for focus)
        if self.thread_id.is_some() {
            defs.extend([
                tool_definition! {
                    name: "focus_task".to_string(),
                    description: "Focus a task, making its content your canvas. Use `patch_task_content` to edit it.".to_string(),
                    params: vec![ToolParam::typed("task_id", "str")],
                    returns: "dict".to_string(),
                    examples: vec![],
                    enabled: true,
                    injected: true,
                },
                tool_definition! {
                    name: "unfocus_task".to_string(),
                    description: "Remove task focus. The canvas detaches from the task.".to_string(),
                    params: vec![],
                    returns: "dict".to_string(),
                    examples: vec![],
                    enabled: true,
                    injected: true,
                },
                tool_definition! {
                    name: "patch_task_content".to_string(),
                    description: format!(
                        "Patch the focused task's content (markdown) using a line-based patch.\n\n{}",
                        TEXT_PATCH_INSTRUCTIONS
                    ),
                    params: vec![ToolParam::typed("patch", "str")],
                    returns: "dict".to_string(),
                    examples: vec![],
                    enabled: true,
                    injected: true,
                },
            ]);
        }

        defs
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        match name {
            "list_tasks" => self.list_tasks().await,
            "create_task" => self.create_task(args).await,
            "update_task" => self.update_task(args).await,
            "focus_task" => self.focus_task(args).await,
            "unfocus_task" => self.unfocus_task().await,
            "patch_task_content" => self.patch_task_content(args).await,
            _ => ToolResult::err(json!({ "error": format!("Unknown tool: {name}") })),
        }
    }
}

impl TaskToolProvider {
    async fn list_tasks(&self) -> ToolResult {
        let store = match TaskStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        match store.list_project_tasks(self.project_id).await {
            Ok(tasks) => ToolResult::ok(json!({ "tasks": tasks })),
            Err(e) => ToolResult::err(json!({ "error": e.to_string() })),
        }
    }

    async fn create_task(&self, args: &Value) -> ToolResult {
        let Some(title) = args.get("title").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "title is required" }));
        };
        let content = args.get("content").and_then(|v| v.as_str());
        let store = match TaskStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        match store.create_task(self.project_id, title, content).await {
            Ok(task) => ToolResult::ok(json!({ "task": task })),
            Err(e) => ToolResult::err(json!({ "error": e.to_string() })),
        }
    }

    async fn update_task(&self, args: &Value) -> ToolResult {
        let Some(task_id) = args.get("task_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "task_id is required" }));
        };
        let title = args.get("title").and_then(|v| v.as_str());
        let status = args.get("status").and_then(|v| v.as_str());
        let content = args.get("content").map(|v| v.as_str());
        let store = match TaskStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        // content is Option<Option<&str>>: None = no change, Some(None) = clear, Some(Some(x)) = set
        match store.update_task(task_id, title, status, content).await {
            Ok(task) => ToolResult::ok(json!({ "task": task })),
            Err(e) => ToolResult::err(json!({ "error": e.to_string() })),
        }
    }

    async fn focus_task(&self, args: &Value) -> ToolResult {
        let Some(thread_id) = &self.thread_id else {
            return ToolResult::err(json!({ "error": "focus_task requires a thread context" }));
        };
        let Some(task_id) = args.get("task_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "task_id is required" }));
        };

        // Verify task exists
        let task_store = match TaskStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        let task = match task_store.get_task(task_id).await {
            Ok(t) => t,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };

        // Set focus on thread
        let thread_store = match ShepherdThreadStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        if let Err(e) = thread_store.set_focused_task(thread_id, Some(task_id)).await {
            return ToolResult::err(json!({ "error": e.to_string() }));
        }

        ToolResult::ok(json!({
            "focused": true,
            "task_id": task.id,
            "title": task.title,
            "content": task.content,
        }))
    }

    async fn unfocus_task(&self) -> ToolResult {
        let Some(thread_id) = &self.thread_id else {
            return ToolResult::err(json!({ "error": "unfocus_task requires a thread context" }));
        };
        let thread_store = match ShepherdThreadStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        if let Err(e) = thread_store.set_focused_task(thread_id, None).await {
            return ToolResult::err(json!({ "error": e.to_string() }));
        }
        ToolResult::ok(json!({ "focused": false }))
    }

    async fn patch_task_content(&self, args: &Value) -> ToolResult {
        let Some(thread_id) = &self.thread_id else {
            return ToolResult::err(json!({ "error": "patch_task_content requires a thread context" }));
        };
        let Some(patch) = args.get("patch").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "patch is required" }));
        };

        // Get focused task from thread
        let thread_store = match ShepherdThreadStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        let thread = match thread_store.get_thread(thread_id).await {
            Ok(t) => t,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        let Some(task_id) = &thread.focused_task_id else {
            return ToolResult::err(json!({
                "error": "No task focused. Use focus_task first."
            }));
        };

        // Get current task content
        let task_store = match TaskStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        let task = match task_store.get_task(task_id).await {
            Ok(t) => t,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };

        let current = task.content.as_deref().unwrap_or("");
        let patched = match apply_text_patch(current, patch) {
            Ok(outcome) => outcome,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        // Save patched content
        if let Err(e) = task_store.update_task_content(task_id, &patched.new_text).await {
            return ToolResult::err(json!({ "error": e.to_string() }));
        }

        let mut fields = serde_json::Map::new();
        fields.insert("task_id".to_string(), json!(task_id));
        fields.insert("added".to_string(), json!(patched.added_lines));
        fields.insert("removed".to_string(), json!(patched.removed_lines));
        edit_result_with("Patched task content", fields)
    }
}

fn task_prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "task_tools",
        "Tasks",
        concat!(
            "Tasks are persistent planning documents with a title, status (todo/active/done), and markdown content.\n",
            "Use `list_tasks` to see what's planned. Use `create_task` / `update_task` for management.\n",
            "Use `focus_task(task_id)` to load a task's content as your canvas. ",
            "Use `patch_task_content(patch)` to edit the focused task's content.\n",
            "Task content supports markdown, mermaid code blocks, and [kind:id] node references.",
        ),
    )]
}

pub(super) fn task_tool_plugin_factory(
    project_id: i64,
    thread_id: Option<String>,
) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "task_tools",
        PluginSpec::new()
            .with_tool_provider(
                Arc::new(TaskToolProvider {
                    project_id,
                    thread_id,
                }) as Arc<dyn ToolProvider>,
            )
            .with_prompt_contributor(Arc::new(|_ctx| {
                Box::pin(async { Ok(task_prompt_contributions()) })
            })),
    ))
}
