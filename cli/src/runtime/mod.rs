use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use telos_agent::{
    AgentConfig, ApprovalHandler, DefaultShell, MemoryStore, ModelProvider, ToolRegistry,
};

use crate::cli::SharedOptions;
use crate::config::{self, FileConfig};
use crate::context::{self, ProjectContext};
use crate::diagnostics::{self, DiagnosticsRuntime};

pub(crate) struct PreparedRuntime {
    pub agent_config: AgentConfig,
    pub tools: ToolRegistry,
    pub project_root: Option<PathBuf>,
    pub project_root_or_cwd: PathBuf,
    pub context: ProjectContext,
    pub memory_store: Arc<Mutex<MemoryStore>>,
    pub diagnostics: Option<DiagnosticsRuntime>,
}

pub(crate) fn prepare_runtime(
    options: &SharedOptions,
    file_config: &FileConfig,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<PreparedRuntime> {
    let current_dir = std::env::current_dir()?;
    let cwd = options.cwd.as_deref().unwrap_or(&current_dir);
    let project_root = crate::project::find_project_root(cwd).ok();
    let project_root_or_cwd = project_root.clone().unwrap_or_else(|| cwd.to_path_buf());
    let context = match &project_root {
        Some(root) => context::load_project_context(root),
        None => ProjectContext::empty(),
    };
    let memory_store = crate::memory_runtime::open_memory_store(project_root.as_deref())?;

    let mut agent_config = config::build_agent_config(options, file_config, approval_handler)?;
    let diagnostics = diagnostics::configure_tool_diagnostics(
        &mut agent_config,
        file_config,
        project_root.as_deref(),
    )?;

    let task_manager = task_manager_for_root(&project_root_or_cwd);
    agent_config.task_manager = Some(task_manager.clone());

    let mut tools = ToolRegistry::new();
    let default_shell = resolve_default_shell(file_config);
    telos_agent::register_core_tools_with_shell(&mut tools, default_shell);
    register_cli_task_tools(&mut tools, task_manager);

    telos_agent::register_memory_tools(&mut tools, memory_store.clone());
    agent_config.prompt_assembly = Some(Arc::new(build_prompt_assembly(
        &agent_config,
        &tools,
        &context,
        memory_store.clone(),
    )));

    Ok(PreparedRuntime {
        agent_config,
        tools,
        project_root,
        project_root_or_cwd,
        context,
        memory_store,
        diagnostics,
    })
}

pub(crate) fn register_cli_subagent_tool(
    tools: &mut ToolRegistry,
    agent_config: &AgentConfig,
    provider: Arc<dyn ModelProvider + Send + Sync>,
) -> Result<()> {
    telos_agent::register_subagent_tool(tools, provider, agent_config)?;
    Ok(())
}

pub(crate) fn rebuild_prompt_assembly(runtime: &mut PreparedRuntime) {
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
    memory_store: Arc<Mutex<MemoryStore>>,
) -> telos_agent::PromptAssembly {
    let tools = Arc::new(tools.clone());
    let mut assembly = telos_agent::PromptAssembly::new();
    assembly.add(telos_agent::IdentitySection::new(None));
    assembly.add(telos_agent::ToneStyleSection);
    assembly.add(telos_agent::TaskGuidanceSection);
    assembly.add(telos_agent::SafetySection);
    assembly.add(telos_agent::prompt::PathSection::new(agent_config.path));
    assembly.add(telos_agent::ShellAwareToolUsageSection::new(Arc::clone(&tools)));
    assembly.add(telos_agent::ToolsSection::new(Arc::clone(&tools)));
    assembly.add(telos_agent::prompt::ToolPromptsSection::new(Arc::clone(&tools)));
    if let Some(skills) = agent_config.skill_registry.clone() {
        assembly.add(telos_agent::SkillsSection::new(skills));
    }
    context::append_prompt_context(&mut assembly, context);
    assembly.add(telos_agent::DateSection);
    assembly.add(telos_agent::CwdSection::new(agent_config.cwd.clone()));
    assembly.add(telos_agent::MemorySection::new(memory_store));
    assembly
}

pub(crate) fn task_manager_for_root(project_root_or_cwd: &Path) -> Arc<telos_agent::TaskManager> {
    Arc::new(telos_agent::TaskManager::new(project_root_or_cwd.join(".telos").join("tasks")))
}

pub(crate) fn register_cli_task_tools(
    registry: &mut ToolRegistry,
    task_manager: Arc<telos_agent::TaskManager>,
) {
    telos_agent::register_task_tools(registry, task_manager);
}

pub(crate) fn resolve_default_shell(file_config: &FileConfig) -> DefaultShell {
    file_config
        .agent
        .as_ref()
        .and_then(|agent| agent.default_shell)
        .unwrap_or_else(DefaultShell::current_platform)
}

pub(crate) async fn process_diagnostics(
    runtime: &Option<DiagnosticsRuntime>,
    file_config: &FileConfig,
) {
    if let Some(runtime) = runtime
        && let Err(err) = diagnostics::process_diagnostics(runtime, file_config).await
    {
        tracing::warn!("failed to process diagnostics: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        rebuild_prompt_assembly, register_cli_subagent_tool, register_cli_task_tools,
        resolve_default_shell, task_manager_for_root,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::config::{AgentSection, FileConfig};
    use serde_json::json;
    use telos_agent::{DefaultShell, Message, MockProvider, ToolContext, ToolRegistry};

    fn tool_context() -> ToolContext {
        ToolContext {
            session_id: "test-session".into(),
            turn_id: 0,
            tool_call_id: None,
            cwd: PathBuf::from("."),
            env: HashMap::new(),
            messages: Arc::new(Vec::<Message>::new()),
            progress: None,
            read_file_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            timeout: None,
            max_file_read_bytes: 50 * 1024 * 1024,
        }
    }

    #[tokio::test]
    async fn register_cli_task_tools_registers_all_task_tools_with_shared_state() {
        let temp = tempfile::tempdir().unwrap();
        let mut registry = ToolRegistry::new();

        register_cli_task_tools(&mut registry, task_manager_for_root(temp.path()));

        for name in ["TaskCreate", "TaskGet", "TaskList", "TaskUpdate", "task_output", "task_stop"]
        {
            assert!(registry.get(name).is_ok(), "{name} should be registered");
        }

        let create = registry.get("TaskCreate").unwrap();
        create
            .invoke(
                json!({
                    "subject": "Wire CLI task tools",
                    "description": "Ensure task tools share manager state",
                }),
                tool_context(),
            )
            .await
            .unwrap();

        let list = registry.get("TaskList").unwrap();
        let output = list.invoke(json!({}), tool_context()).await.unwrap();
        let tasks = output.content.as_array().unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["subject"], "Wire CLI task tools");
    }

    #[test]
    fn resolve_default_shell_uses_config_override() {
        let config = FileConfig {
            agent: Some(AgentSection {
                default_shell: Some(DefaultShell::PowerShell),
                ..Default::default()
            }),
            ..FileConfig::default()
        };

        assert_eq!(resolve_default_shell(&config), DefaultShell::PowerShell);
    }

    #[tokio::test]
    async fn cli_runtime_registers_subagent_tool_and_prompt() {
        let temp = tempfile::tempdir().unwrap();
        let options = crate::cli::SharedOptions {
            cwd: Some(temp.path().to_path_buf()),
            ..crate::cli::SharedOptions::default()
        };
        let mut runtime = super::prepare_runtime(&options, &FileConfig::default(), None).unwrap();
        let provider = Arc::new(MockProvider::new(vec![]));

        assert!(runtime.agent_config.task_manager.is_some());
        register_cli_subagent_tool(&mut runtime.tools, &runtime.agent_config, provider).unwrap();
        rebuild_prompt_assembly(&mut runtime);

        assert!(runtime.tools.get("subagent").is_ok());
        let prompt = runtime.agent_config.prompt_assembly.unwrap().build().await;
        assert!(prompt.contains("Subagent"));
        assert!(prompt.contains("subagent_type"));
    }
}
