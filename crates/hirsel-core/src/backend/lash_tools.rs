use std::collections::BTreeMap;
use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::tools::{ApplyPatchTool, FetchUrl, Glob, Grep, Ls, ReadFilePluginFactory, WebSearch};
use lash::{
    attach_mcp_servers, BuiltinToolResultProjectionPluginFactory, DynamicToolProvider,
    FsInstructionSource, InstructionSource, McpServerConfig, PluginSpec, PromptContribution,
    ToolProvider,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmbeddedToolPreset {
    General,
    Shepherd,
}

pub(crate) struct EmbeddedCustomToolPlugin {
    pub(crate) id: &'static str,
    pub(crate) provider: Arc<dyn ToolProvider>,
    pub(crate) prompt_contributions: Option<fn() -> Vec<PromptContribution>>,
}

fn builtin_projection_plugin_factory() -> Arc<dyn PluginFactory> {
    Arc::new(BuiltinToolResultProjectionPluginFactory::default()) as Arc<dyn PluginFactory>
}

fn shell_plugin_factory(shell_tool_provider: Arc<dyn ToolProvider>) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "shell",
        PluginSpec::new().with_tool_provider(shell_tool_provider),
    )) as Arc<dyn PluginFactory>
}

fn apply_patch_plugin_factory() -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "apply_patch",
        PluginSpec::new().with_tool_provider(Arc::new(ApplyPatchTool) as Arc<dyn ToolProvider>),
    )) as Arc<dyn PluginFactory>
}

fn read_file_plugin_factory(
    instruction_source: Option<Arc<dyn InstructionSource>>,
) -> Arc<dyn PluginFactory> {
    Arc::new(ReadFilePluginFactory::new(instruction_source)) as Arc<dyn PluginFactory>
}

fn glob_plugin_factory() -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "glob",
        PluginSpec::new().with_tool_provider(Arc::new(Glob) as Arc<dyn ToolProvider>),
    )) as Arc<dyn PluginFactory>
}

fn grep_plugin_factory() -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "grep",
        PluginSpec::new().with_tool_provider(Arc::new(Grep) as Arc<dyn ToolProvider>),
    )) as Arc<dyn PluginFactory>
}

fn ls_plugin_factory() -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "ls",
        PluginSpec::new().with_tool_provider(Arc::new(Ls) as Arc<dyn ToolProvider>),
    )) as Arc<dyn PluginFactory>
}

fn custom_tool_plugin_factory(custom_plugin: EmbeddedCustomToolPlugin) -> Arc<dyn PluginFactory> {
    let mut spec = PluginSpec::new().with_tool_provider(custom_plugin.provider);
    if let Some(make_contributions) = custom_plugin.prompt_contributions {
        spec = spec.with_prompt_contributor(Arc::new(move |_ctx| {
            let contributions = make_contributions();
            Box::pin(async move { Ok(contributions) })
        }));
    }
    Arc::new(StaticPluginFactory::new(custom_plugin.id, spec)) as Arc<dyn PluginFactory>
}

fn append_web_plugins(factories: &mut Vec<Arc<dyn PluginFactory>>, tavily_api_key: Option<String>) {
    if let Some(tavily_api_key) = tavily_api_key.filter(|value| !value.trim().is_empty()) {
        let search_key = tavily_api_key.clone();
        factories.push(Arc::new(StaticPluginFactory::new(
            "search_web",
            PluginSpec::new()
                .with_tool_provider(Arc::new(WebSearch::new(search_key)) as Arc<dyn ToolProvider>),
        )) as Arc<dyn PluginFactory>);
        factories.push(Arc::new(StaticPluginFactory::new(
            "fetch_url",
            PluginSpec::new().with_tool_provider(
                Arc::new(FetchUrl::new(tavily_api_key)) as Arc<dyn ToolProvider>
            ),
        )) as Arc<dyn PluginFactory>);
    }
}

fn standard_tool_plugin_factories(
    shell_tool_provider: Arc<dyn ToolProvider>,
    tavily_api_key: Option<String>,
) -> Vec<Arc<dyn PluginFactory>> {
    let instruction_source: Arc<dyn InstructionSource> = Arc::new(FsInstructionSource::new());
    let mut factories = vec![
        builtin_projection_plugin_factory(),
        shell_plugin_factory(shell_tool_provider),
        apply_patch_plugin_factory(),
        read_file_plugin_factory(Some(instruction_source)),
        glob_plugin_factory(),
        grep_plugin_factory(),
        ls_plugin_factory(),
    ];
    append_web_plugins(&mut factories, tavily_api_key);
    factories
}

fn shepherd_tool_plugin_factories(
    shell_tool_provider: Arc<dyn ToolProvider>,
    tavily_api_key: Option<String>,
    custom_plugin: EmbeddedCustomToolPlugin,
) -> Vec<Arc<dyn PluginFactory>> {
    let mut factories = vec![
        builtin_projection_plugin_factory(),
        shell_plugin_factory(shell_tool_provider),
        apply_patch_plugin_factory(),
        glob_plugin_factory(),
        custom_tool_plugin_factory(custom_plugin),
    ];
    append_web_plugins(&mut factories, tavily_api_key);
    factories
}

pub(crate) fn embedded_tool_plugin_factories(
    preset: EmbeddedToolPreset,
    custom_plugin: Option<EmbeddedCustomToolPlugin>,
    shell_tool_provider: Arc<dyn ToolProvider>,
    tavily_api_key: Option<String>,
) -> Vec<Arc<dyn PluginFactory>> {
    match preset {
        EmbeddedToolPreset::General => {
            standard_tool_plugin_factories(shell_tool_provider, tavily_api_key)
        }
        EmbeddedToolPreset::Shepherd => shepherd_tool_plugin_factories(
            shell_tool_provider,
            tavily_api_key,
            custom_plugin.expect("custom tool preset requires a custom tool plugin"),
        ),
    }
}

pub(crate) async fn attach_embedded_mcp_servers(
    dynamic_tools: &DynamicToolProvider,
    servers: &BTreeMap<String, McpServerConfig>,
) -> Result<(), String> {
    attach_mcp_servers(dynamic_tools, servers)
        .await
        .map_err(|e| format!("failed to attach MCP servers: {}", e))
}
