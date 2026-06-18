use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::io::Write;
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
    let (mut session, tools, provider) =
        setup_session(options, approval_handler).context("failed to set up agent session")?;

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
    let (mut session, tools, provider) = setup_session(options, approval_handler.clone())
        .context("failed to set up agent session")?;

    eprintln!("telos chat — type /exit or /quit to leave");
    eprintln!("Available commands: /exit, /quit, /reset, /tools");
    eprintln!();

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());

    loop {
        eprint!("> ");
        let _ = std::io::stderr().flush();

        let mut line = String::new();
        let bytes = tokio::io::AsyncBufReadExt::read_line(&mut stdin, &mut line)
            .await
            .context("failed to read from stdin")?;

        if bytes == 0 {
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        match handle_repl_command(input) {
            ReplCommand::Exit => break,
            ReplCommand::Reset => {
                // Re-create session to drop history, keeping the same approval handler.
                let (new_session, _, _) = setup_session(options, approval_handler.clone())
                    .context("failed to reset session")?;
                session = new_session;
                eprintln!("Session reset.");
                continue;
            }
            ReplCommand::Tools => {
                eprintln!("Registered tools:");
                for (name, _) in tools.iter() {
                    eprintln!("  - {name}");
                }
                continue;
            }
            ReplCommand::Chat(prompt) => match &provider {
                ResolvedProvider::Kimi(p) => {
                    run_with_provider(&mut session, p, &tools, prompt).await?;
                }
                ResolvedProvider::DeepSeek(p) => {
                    run_with_provider(&mut session, p, &tools, prompt).await?;
                }
                ResolvedProvider::Mock(_) => {
                    eprintln!("Note: using mock provider; no real model call is made.");
                    let mock = MockProvider::new(vec![CompletionResponse {
                        message: Message::assistant(
                            "Mock provider has no real response configured.",
                        ),
                        stop_reason: StopReason::EndTurn,
                        usage: None,
                    }]);
                    run_with_provider(&mut session, &mock, &tools, prompt).await?;
                }
            },
        }
    }

    Ok(())
}

fn setup_session(
    options: &SharedOptions,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<(AgentSession, ToolRegistry, ResolvedProvider)> {
    let config = build_agent_config(options, approval_handler)?;
    let session = AgentSession::new(config).context("failed to create agent session")?;
    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);
    let provider = build_provider(options)?;
    Ok((session, tools, provider))
}

enum ReplCommand {
    Exit,
    Reset,
    Tools,
    Chat(String),
}

fn handle_repl_command(input: &str) -> ReplCommand {
    match input {
        "/exit" | "/quit" => ReplCommand::Exit,
        "/reset" => ReplCommand::Reset,
        "/tools" => ReplCommand::Tools,
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
