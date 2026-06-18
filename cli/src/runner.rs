use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::pin::pin;
use std::sync::Arc;
use telos_agent::{
    AgentSession, ApprovalHandler, CompletionResponse, Message, MockProvider, StopReason,
    ToolRegistry,
};

use crate::cli::SharedOptions;
use crate::config::{ResolvedProvider, build_agent_config, build_provider};

pub async fn run_single(
    options: &SharedOptions,
    prompt: String,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let config = build_agent_config(options, approval_handler)?;
    let mut session = AgentSession::new(config).context("failed to create agent session")?;
    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);

    let provider = build_provider(options)?;

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
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let config = build_agent_config(options, approval_handler)?;
    let provider = crate::build_erased_provider(options)?;
    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);

    let current_dir = std::env::current_dir()?;
    let cwd = options.cwd.as_deref().unwrap_or(&current_dir);
    let project_root = crate::project::find_project_root(cwd).ok();
    let ctx = match &project_root {
        Some(root) => crate::context::load_project_context(root),
        None => crate::context::ProjectContext::empty(),
    };

    let status =
        crate::context::build_status_text(options.model.as_deref(), project_root.as_deref(), &ctx);

    crate::tui::run(config, provider, tools, status).await
}

/// Represents a parsed slash command or chat input.
#[derive(Debug, PartialEq)]
pub enum ReplCommand {
    Exit,
    Reset,
    Tools,
    Clear,
    Help,
    Add(String),
    Drop(String),
    Model(String),
    Chat(String),
}

/// Parse a line of user input into a `ReplCommand`.
pub fn parse_repl_command(input: &str) -> ReplCommand {
    let input = input.trim();
    match input {
        "/exit" | "/quit" => ReplCommand::Exit,
        "/reset" => ReplCommand::Reset,
        "/tools" => ReplCommand::Tools,
        "/clear" => ReplCommand::Clear,
        "/help" => ReplCommand::Help,
        s if s.starts_with("/add ") => ReplCommand::Add(s[5..].trim().to_string()),
        s if s.starts_with("/drop ") => ReplCommand::Drop(s[6..].trim().to_string()),
        s if s.starts_with("/model ") => ReplCommand::Model(s[7..].trim().to_string()),
        _ => ReplCommand::Chat(input.to_string()),
    }
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
