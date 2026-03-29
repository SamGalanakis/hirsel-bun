use std::collections::BTreeMap;
use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::tools::{
    ApplyPatchTool, FetchUrl, Glob, Grep, Ls, ReadFilePluginFactory, StandardShell, WebSearch,
};
use lash::{
    attach_mcp_servers, BuiltinToolResultProjectionPluginFactory, DynamicToolProvider,
    FsInstructionSource, InstructionSource, McpServerConfig, PluginSpec, PromptContribution,
    ToolProvider,
};

fn shell_prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "### Command Execution\nUse `exec_command` for one-shot commands and for starting long-lived processes. If it returns `session_id`, continue that same process with `write_stdin`; otherwise the command already exited. For services or background daemons, prefer startup patterns that survive after the tool call returns, then verify readiness from a fresh command before concluding.\n\n### Git Safety\nDo not revert user changes you did not make. Avoid destructive git commands unless explicitly requested.",
    )]
}

fn base_tool_plugin_factories(
    instruction_source: Option<Arc<dyn InstructionSource>>,
    tavily_api_key: Option<String>,
) -> Vec<Arc<dyn PluginFactory>> {
    let mut factories: Vec<Arc<dyn PluginFactory>> = vec![
        Arc::new(BuiltinToolResultProjectionPluginFactory::default()) as Arc<dyn PluginFactory>,
        Arc::new(StaticPluginFactory::new(
            "shell",
            PluginSpec::new()
                .with_tool_provider(Arc::new(StandardShell::new()) as Arc<dyn ToolProvider>)
                .with_prompt_contributor(Arc::new(move |_ctx| {
                    Box::pin(async move { Ok(shell_prompt_contributions()) })
                })),
        )) as Arc<dyn PluginFactory>,
        Arc::new(StaticPluginFactory::new(
            "apply_patch",
            PluginSpec::new().with_tool_provider(Arc::new(ApplyPatchTool) as Arc<dyn ToolProvider>),
        )) as Arc<dyn PluginFactory>,
        Arc::new(ReadFilePluginFactory::new(instruction_source)) as Arc<dyn PluginFactory>,
        Arc::new(StaticPluginFactory::new(
            "glob",
            PluginSpec::new().with_tool_provider(Arc::new(Glob) as Arc<dyn ToolProvider>),
        )) as Arc<dyn PluginFactory>,
        Arc::new(StaticPluginFactory::new(
            "grep",
            PluginSpec::new().with_tool_provider(Arc::new(Grep) as Arc<dyn ToolProvider>),
        )) as Arc<dyn PluginFactory>,
        Arc::new(StaticPluginFactory::new(
            "ls",
            PluginSpec::new().with_tool_provider(Arc::new(Ls) as Arc<dyn ToolProvider>),
        )) as Arc<dyn PluginFactory>,
    ];

    if let Some(key) = tavily_api_key {
        let search_key = key.clone();
        factories.push(Arc::new(StaticPluginFactory::new(
            "search_web",
            PluginSpec::new()
                .with_tool_provider(Arc::new(WebSearch::new(search_key)) as Arc<dyn ToolProvider>),
        )) as Arc<dyn PluginFactory>);
        factories.push(Arc::new(StaticPluginFactory::new(
            "fetch_url",
            PluginSpec::new()
                .with_tool_provider(Arc::new(FetchUrl::new(key)) as Arc<dyn ToolProvider>),
        )) as Arc<dyn PluginFactory>);
    }

    factories
}

pub(crate) fn embedded_tool_plugin_factories(
    custom_plugin_id: &'static str,
    custom_tool_provider: Arc<dyn ToolProvider>,
    tavily_api_key: Option<String>,
) -> Vec<Arc<dyn PluginFactory>> {
    let instruction_source: Arc<dyn InstructionSource> = Arc::new(FsInstructionSource::new());
    let mut factories = base_tool_plugin_factories(Some(instruction_source), tavily_api_key);
    factories.push(Arc::new(StaticPluginFactory::new(
        custom_plugin_id,
        PluginSpec::new().with_tool_provider(custom_tool_provider),
    )) as Arc<dyn PluginFactory>);
    factories
}

pub(crate) async fn attach_embedded_mcp_servers(
    dynamic_tools: &DynamicToolProvider,
    servers: &BTreeMap<String, McpServerConfig>,
) -> Result<(), String> {
    attach_mcp_servers(dynamic_tools, servers)
        .await
        .map_err(|e| format!("failed to attach MCP servers: {}", e))
}
