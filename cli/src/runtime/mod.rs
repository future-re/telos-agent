use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use telos_agent::{AgentConfig, ApprovalHandler, MemoryStore, ToolRegistry};

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

    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);
    register_cli_task_tools(&mut tools, &project_root_or_cwd);

    let mut assembly = context::build_prompt_assembly(&context);
    crate::memory_runtime::register_memory_runtime(&mut tools, &mut assembly, memory_store.clone());
    agent_config.prompt_assembly = Some(Arc::new(assembly));

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

pub(crate) fn register_cli_task_tools(registry: &mut ToolRegistry, project_root_or_cwd: &Path) {
    let task_dir = project_root_or_cwd.join(".telos").join("tasks");
    let task_manager = Arc::new(telos_agent::TaskManager::new(task_dir));
    telos_agent::register_task_tools(registry, task_manager);
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
    use super::register_cli_task_tools;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::json;
    use telos_agent::{Message, ToolContext, ToolRegistry};

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

        register_cli_task_tools(&mut registry, temp.path());

        for name in ["TaskCreate", "TaskGet", "TaskList", "TaskUpdate"] {
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
}
