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
    agent_config.prompt_assembly = Some(Arc::new(build_prompt_assembly(
        &agent_config,
        &tools,
        &context,
        memory_store.clone(),
    )));

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
    runtime.agent_config.prompt_assembly = Some(Arc::new(build_prompt_assembly(
        &runtime.agent_config,
        &runtime.tools,
        &runtime.context,
        runtime.memory_store.clone(),
    )));
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
