use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::pin::pin;
use telos_agent::{
    AgentSession, CompletionResponse, Message, MockProvider, StopReason, ToolRegistry,
};

use crate::cli::SharedOptions;
use crate::config::{ResolvedProvider, build_agent_config, build_provider};

pub async fn run_single(options: &SharedOptions, prompt: String) -> Result<()> {
    let config = build_agent_config(options)?;
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
