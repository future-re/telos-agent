use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use telos_agent::{
    AgentConfig, ApprovalHandler, DefaultShell, MemoryStore, ModelProvider, ToolRegistry,
};

use crate::config::{self, FileConfig};
use crate::context::{self, ProjectContext};
use crate::memory_runtime;
use crate::options::SharedOptions;
use crate::project;

pub struct PreparedRuntime {
    pub agent_config: AgentConfig,
    pub tools: ToolRegistry,
    pub project_root: Option<PathBuf>,
    pub context: ProjectContext,
    pub memory_store: Arc<Mutex<MemoryStore>>,
}

pub fn prepare_runtime(
    options: &SharedOptions,
    file_config: &FileConfig,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<PreparedRuntime> {
    let current_dir = std::env::current_dir()?;
    let cwd = options.cwd.as_deref().unwrap_or(&current_dir);
    let project_root = project::find_project_root(cwd).ok();
    let project_root_or_cwd = project_root.clone().unwrap_or_else(|| cwd.to_path_buf());
    let context = match &project_root {
        Some(root) => context::load_project_context(root),
        None => ProjectContext::empty(),
    };
    let memory_store = memory_runtime::open_memory_store(project_root.as_deref())?;

    let mut agent_config = config::build_agent_config(options, file_config, approval_handler)?;
    let task_manager = task_manager_for_root(&project_root_or_cwd);
    agent_config.task_manager = Some(task_manager.clone());

    let mut tools = ToolRegistry::new();
    let default_shell = resolve_default_shell(file_config);
    telos_agent::register_core_tools_with_shell(&mut tools, default_shell);
    register_task_tools(&mut tools, task_manager);
    telos_agent::register_memory_tools(&mut tools, memory_store.clone());
    agent_config.memory_injector =
        Some(Arc::new(telos_agent::MemoryInjector::new(memory_store.clone())));
    if let Some(skill_registry) = agent_config.skill_registry.clone() {
        agent_config.skill_injector =
            Some(Arc::new(telos_agent::SkillInjector::new(skill_registry)));
    }

    // Load project MCP servers
    let mcp_config_path = project_root_or_cwd.join(".tiny-agent").join("mcp.json");
    let mcp_manager = telos_agent::McpManager::load_config(&mcp_config_path)
        .unwrap_or_else(|e| {
            if mcp_config_path.exists() {
                tracing::warn!(path = %mcp_config_path.display(), error = %e, "failed to load MCP config");
            }
            telos_agent::McpManager::new(HashMap::new())
        });

    // Create plugin registry and discover installed plugins
    let plugin_registry = create_plugin_registry(&project_root_or_cwd);
    agent_config.plugin_registry = plugin_registry.clone();

    // Prepare registries for plugin apply
    let hooks = hook_registry_for_config(&agent_config);
    let skills = agent_config.skill_registry.clone().map(|r| (*r).clone()).unwrap_or_default();
    let prompt = build_prompt_assembly(&agent_config, &tools, &context, memory_store.clone());

    // Apply enabled plugins — always get the registries back
    let (mut tools, hooks, skills, mcp_manager, mut prompt, plugin_result) =
        agent_config.apply_plugins(tools, hooks, skills, mcp_manager, prompt);
    agent_config.hooks = Arc::new(hooks);
    agent_config.skill_registry = Some(Arc::new(skills));
    if let Err(errors) = plugin_result {
        for error in &errors {
            tracing::warn!(?error, "plugin apply error");
        }
    }

    // Connect all MCP servers and register their tools
    let mcp_manager = Arc::new(mcp_manager);
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            mcp_manager.connect_all().await;
            for (server_id, mcp_tool) in mcp_manager.all_tools().await {
                let bridge =
                    telos_agent::McpToolBridge::new(server_id, mcp_tool, mcp_manager.clone());
                tools.register(bridge);
            }
        });
    });
    agent_config.mcp_manager = Some(mcp_manager.clone());

    // Add MCP section to the plugin-applied prompt assembly
    prompt.add(telos_agent::McpSection::new(mcp_manager.clone()));
    agent_config.prompt_assembly = Some(Arc::new(prompt));

    Ok(PreparedRuntime { agent_config, tools, project_root, context, memory_store })
}

pub fn register_subagent_tool(
    tools: &mut ToolRegistry,
    agent_config: &AgentConfig,
    provider: Arc<dyn ModelProvider + Send + Sync>,
) -> Result<()> {
    telos_agent::register_subagent_tool(tools, provider, agent_config)?;
    Ok(())
}

pub fn rebuild_prompt_assembly(runtime: &mut PreparedRuntime) {
    let mut prompt = build_prompt_assembly(
        &runtime.agent_config,
        &runtime.tools,
        &runtime.context,
        runtime.memory_store.clone(),
    );

    // Re-apply plugin prompt sections
    if let Some(ref registry) = runtime.agent_config.plugin_registry {
        registry.apply_prompt_sections(&mut prompt);
    }

    if let Some(mcp_manager) = runtime.agent_config.mcp_manager.clone() {
        prompt.add(telos_agent::McpSection::new(mcp_manager));
    }

    runtime.agent_config.prompt_assembly = Some(Arc::new(prompt));
}

fn build_prompt_assembly(
    agent_config: &AgentConfig,
    tools: &ToolRegistry,
    context: &ProjectContext,
    _memory_store: Arc<Mutex<MemoryStore>>,
) -> telos_agent::PromptAssembly {
    let mut assembly = telos_agent::prompt::default_coding_assembly_for_profile(
        Arc::new(tools.clone()),
        agent_config.cwd.clone(),
        agent_config.skill_registry.clone(),
        agent_config.path,
        agent_config.prompt_profile,
    );
    context::append_prompt_context(&mut assembly, context);
    assembly
}

fn hook_registry_for_config(agent_config: &AgentConfig) -> telos_agent::HookRegistry {
    agent_config.hooks.as_ref().clone()
}

fn create_plugin_registry(project_root_or_cwd: &Path) -> Option<Arc<telos_agent::PluginRegistry>> {
    let telos_dir = project_root_or_cwd.join(".telos");
    let plugins_root = telos_dir.join("plugins");
    let mut registry = telos_agent::PluginRegistry::new(&plugins_root);

    let installed_dir = registry.installed_dir();
    if let Err(e) = std::fs::create_dir_all(&installed_dir) {
        tracing::debug!(path = %installed_dir.display(), error = %e, "failed to create plugin installed dir");
    }

    let discovered = match registry.discover_installed() {
        Ok(ids) => {
            tracing::info!(count = ids.len(), "discovered installed plugins");
            ids
        }
        Err(e) => {
            tracing::debug!(error = %e, "failed to discover installed plugins");
            Vec::new()
        }
    };

    if discovered.is_empty() {
        return None;
    }

    if let Err(e) = registry.load_state() {
        tracing::warn!(error = %e, "failed to load plugin state");
    }

    Some(Arc::new(registry))
}

pub fn task_manager_for_root(project_root_or_cwd: &Path) -> Arc<telos_agent::TaskManager> {
    Arc::new(telos_agent::TaskManager::new(project_root_or_cwd.join(".telos").join("tasks")))
}

pub fn register_task_tools(
    registry: &mut ToolRegistry,
    task_manager: Arc<telos_agent::TaskManager>,
) {
    telos_agent::register_task_tools(registry, task_manager);
}

pub fn resolve_default_shell(file_config: &FileConfig) -> DefaultShell {
    file_config
        .agent
        .as_ref()
        .and_then(|agent| agent.default_shell)
        .unwrap_or_else(DefaultShell::current_platform)
}
