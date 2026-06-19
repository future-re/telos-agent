use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use telos_agent::{
    AgentSession, ApprovalHandler, CompletionResponse, MemoryStore, Message, MockProvider,
    StopReason, ToolRegistry,
};

use crate::cli::SharedOptions;
use crate::config::{self, FileConfig, ResolvedProvider};

pub(crate) fn register_cli_task_tools(registry: &mut ToolRegistry, project_root_or_cwd: &Path) {
    let task_dir = project_root_or_cwd.join(".telos").join("tasks");
    let task_manager = Arc::new(telos_agent::TaskManager::new(task_dir));
    telos_agent::register_task_tools(registry, task_manager);
}

pub async fn run_single(
    options: &SharedOptions,
    config: &FileConfig,
    onboarding: Option<crate::onboarding::OnboardingResult>,
    prompt: String,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let current_dir = std::env::current_dir()?;
    let cwd = options.cwd.as_deref().unwrap_or(&current_dir);
    let project_root = crate::project::find_project_root(cwd).ok();
    let ctx = match &project_root {
        Some(root) => crate::context::load_project_context(root),
        None => crate::context::ProjectContext::empty(),
    };
    let memory_store = crate::memory_runtime::open_memory_store(project_root.as_deref())?;

    let mut agent_config = config::build_agent_config(options, config, approval_handler)?;
    let _diagnostics_runtime = crate::diagnostics::configure_tool_diagnostics(
        &mut agent_config,
        config,
        project_root.as_deref(),
    )?;
    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);
    register_cli_task_tools(&mut tools, project_root.as_deref().unwrap_or(cwd));
    let mut assembly = crate::context::build_prompt_assembly(&ctx);
    crate::memory_runtime::register_memory_runtime(&mut tools, &mut assembly, memory_store.clone());
    // CodeQL startup analysis (background).
    let codeql_cfg_run_single = crate::codeql_runtime::codeql_config_from_file(config);
    let codeql_runtime_run_single = crate::codeql_runtime::register_codeql(
        &mut tools,
        &mut assembly,
        memory_store.clone(),
        codeql_cfg_run_single,
        project_root.clone().unwrap_or_else(|| cwd.to_path_buf()),
    );
    if let Some(runtime) = codeql_runtime_run_single {
        tokio::spawn(async move {
            let report = runtime.run_startup_analysis().await;
            tracing::info!(?report, "CodeQL startup analysis complete");
        });
    }
    agent_config.prompt_assembly = Some(Arc::new(assembly));

    let mut session = AgentSession::new(agent_config).context("failed to create agent session")?;

    let provider = if let Some(ref onb) = onboarding {
        config::build_provider_from_onboarding(onb)?
    } else {
        config::build_provider(options, config)?
    };

    match provider {
        ResolvedProvider::DeepSeek(p) => {
            run_with_provider(&mut session, &p, &tools, prompt, memory_store.clone()).await?;
        }
        ResolvedProvider::Routed(p) => {
            run_with_provider(&mut session, &p, &tools, prompt, memory_store.clone()).await?;
        }
        ResolvedProvider::Mock(_) => {
            eprintln!("Note: using mock provider; no real model call is made.");
            let mock = MockProvider::new(vec![CompletionResponse {
                message: Message::assistant("Mock provider has no real response configured."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            }]);
            run_with_provider(&mut session, &mock, &tools, prompt, memory_store.clone()).await?;
        }
    }

    if let Some(runtime) = &_diagnostics_runtime
        && let Err(err) = crate::diagnostics::process_diagnostics(runtime, config).await
    {
        tracing::warn!("failed to process diagnostics: {err}");
    }

    Ok(())
}

pub async fn run_chat(
    options: &SharedOptions,
    config: &FileConfig,
    onboarding: Option<crate::onboarding::OnboardingResult>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let mut agent_config = config::build_agent_config(options, config, approval_handler)?;
    let provider = if let Some(ref onb) = onboarding {
        crate::build_erased_from_onboarding(onb)?
    } else {
        crate::build_erased_provider(options, config)?
    };
    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);

    let current_dir = std::env::current_dir()?;
    let cwd = options.cwd.as_deref().unwrap_or(&current_dir);
    let project_root = crate::project::find_project_root(cwd).ok();
    register_cli_task_tools(&mut tools, project_root.as_deref().unwrap_or(cwd));
    let _diagnostics_runtime = crate::diagnostics::configure_tool_diagnostics(
        &mut agent_config,
        config,
        project_root.as_deref(),
    )?;
    let ctx = match &project_root {
        Some(root) => crate::context::load_project_context(root),
        None => crate::context::ProjectContext::empty(),
    };

    // Inject the loaded context into the agent's prompt assembly.
    let memory_store = crate::memory_runtime::open_memory_store(project_root.as_deref())?;
    let mut assembly = crate::context::build_prompt_assembly(&ctx);
    crate::memory_runtime::register_memory_runtime(&mut tools, &mut assembly, memory_store.clone());
    // CodeQL startup analysis (background).
    let codeql_cfg_chat = crate::codeql_runtime::codeql_config_from_file(config);
    let codeql_runtime_chat = crate::codeql_runtime::register_codeql(
        &mut tools,
        &mut assembly,
        memory_store.clone(),
        codeql_cfg_chat,
        project_root.clone().unwrap_or_else(|| cwd.to_path_buf()),
    );
    if let Some(runtime) = codeql_runtime_chat {
        tokio::spawn(async move {
            let report = runtime.run_startup_analysis().await;
            tracing::info!(?report, "CodeQL startup analysis complete");
        });
    }
    agent_config.prompt_assembly = Some(Arc::new(assembly));

    let status =
        crate::context::build_status_text(options.model.as_deref(), project_root.as_deref(), &ctx);

    let auto_mode = config.auto_mode.unwrap_or(false);
    let result = crate::tui::run(
        agent_config,
        provider,
        tools,
        status,
        project_root.as_deref(),
        project_root.as_deref().unwrap_or(cwd),
        auto_mode,
        memory_store,
        crate::tui::app::ModelSwitchConfig {
            deepseek_api_key: crate::deepseek_api_key_for_switch(
                options,
                config,
                onboarding.as_ref(),
            ),
        },
        crate::tui::app::TuiLayoutSettings::from_density(
            config.tui.as_ref().and_then(|tui| tui.density).unwrap_or_default(),
        ),
    )
    .await;
    if let Some(runtime) = &_diagnostics_runtime
        && let Err(err) = crate::diagnostics::process_diagnostics(runtime, config).await
    {
        tracing::warn!("failed to process diagnostics: {err}");
    }
    result
}

async fn run_with_provider<P: telos_agent::ModelProvider>(
    session: &mut AgentSession,
    provider: &P,
    tools: &ToolRegistry,
    prompt: String,
    memory_store: Arc<Mutex<MemoryStore>>,
) -> Result<()> {
    crate::memory_runtime::record_user_preference(&memory_store, &prompt).await;
    let mut stream = pin!(session.run_turn_stream(provider, tools, prompt));
    let mut printed = String::new();
    let mut tool_details: HashMap<String, String> = HashMap::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(telos_agent::TurnEvent::AssistantDelta { text }) => {
                print!("{text}");
                printed.push_str(&text);
            }
            Ok(telos_agent::TurnEvent::ToolCall { tool_call_id, name, detail }) => {
                tool_details.insert(tool_call_id, detail);
                eprintln!("\n[tool: {name}]");
            }
            Ok(telos_agent::TurnEvent::ToolCompleted { tool_call_id, name, is_error }) => {
                if is_error {
                    eprintln!("[tool {name} failed]");
                } else {
                    eprintln!("[tool {name} completed]");
                    crate::memory_runtime::record_successful_tool(
                        &memory_store,
                        &name,
                        &tool_call_id,
                        tool_details.get(&tool_call_id).map(String::as_str),
                    )
                    .await;
                }
            }
            Ok(telos_agent::TurnEvent::ToolResult(message)) => {
                for result in message.tool_results_iter() {
                    if result.is_error {
                        crate::memory_runtime::record_tool_error(
                            &memory_store,
                            result,
                            tool_details.get(&result.tool_call_id).map(String::as_str),
                        )
                        .await;
                    }
                }
            }
            Ok(telos_agent::TurnEvent::TurnFinished { final_text, .. }) => {
                if !final_text.is_empty() && !printed.ends_with(&final_text) {
                    print!("{final_text}");
                }
                println!();
            }
            Err(e) => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
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
