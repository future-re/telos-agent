use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::pin::pin;
use std::sync::Arc;
use telos_agent::{
    AgentSession, ApprovalHandler, CompletionResponse, Message, MockProvider, StopReason,
    ToolRegistry,
};

use crate::cli::SharedOptions;
use crate::config::{self, FileConfig, ResolvedProvider};

pub async fn run_single(
    options: &SharedOptions,
    config: &FileConfig,
    onboarding: Option<crate::onboarding::OnboardingResult>,
    prompt: String,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let agent_config = config::build_agent_config(options, config, approval_handler)?;
    let mut session = AgentSession::new(agent_config).context("failed to create agent session")?;
    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);

    let provider = if let Some(ref onb) = onboarding {
        config::build_provider_from_onboarding(onb)?
    } else {
        config::build_provider(options, config)?
    };

    match provider {
        ResolvedProvider::Kimi(p) => {
            run_with_provider(&mut session, &p, &tools, prompt).await?;
        }
        ResolvedProvider::DeepSeek(p) => {
            run_with_provider(&mut session, &p, &tools, prompt).await?;
        }
        ResolvedProvider::Mock(_) => {
            eprintln!("Note: using mock provider; no real model call is made.");
            let mock = MockProvider::new(vec![CompletionResponse {
                message: Message::assistant("Mock provider has no real response configured."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            }]);
            run_with_provider(&mut session, &mock, &tools, prompt).await?;
        }
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
    let ctx = match &project_root {
        Some(root) => crate::context::load_project_context(root),
        None => crate::context::ProjectContext::empty(),
    };

    // Inject the loaded context into the agent's prompt assembly.
    let assembly = crate::context::build_prompt_assembly(&ctx);
    agent_config.prompt_assembly = Some(Arc::new(assembly));

    let status =
        crate::context::build_status_text(options.model.as_deref(), project_root.as_deref(), &ctx);

    crate::tui::run(agent_config, provider, tools, status, project_root.as_deref()).await
}

async fn run_with_provider<P: telos_agent::ModelProvider>(
    session: &mut AgentSession,
    provider: &P,
    tools: &ToolRegistry,
    prompt: String,
) -> Result<()> {
    let mut stream = pin!(session.run_turn_stream(provider, tools, prompt));
    let mut printed = String::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(telos_agent::TurnEvent::AssistantDelta { text }) => {
                print!("{text}");
                printed.push_str(&text);
            }
            Ok(telos_agent::TurnEvent::ToolCall { name, .. }) => {
                eprintln!("\n[tool: {name}]");
            }
            Ok(telos_agent::TurnEvent::ToolCompleted { name, is_error, .. }) => {
                if is_error {
                    eprintln!("[tool {name} failed]");
                } else {
                    eprintln!("[tool {name} completed]");
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
