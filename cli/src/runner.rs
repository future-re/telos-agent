use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::io::{self, Write};
use std::pin::pin;
use std::sync::{Arc, Mutex};
use telos_agent::{
    AgentSession, ApprovalHandler, CompletionResponse, MemoryStore, Message, MockProvider,
    StopReason, TokenUsage, ToolRegistry,
};

use crate::billing::CostCalculator;
use crate::cli::SharedOptions;
use crate::config::{self, FileConfig, ResolvedProvider};

pub async fn run_single(
    options: &SharedOptions,
    config: &FileConfig,
    onboarding: Option<crate::onboarding::OnboardingResult>,
    prompt: String,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let mut runtime = crate::runtime::prepare_runtime(options, config, approval_handler)?;

    let provider = if let Some(ref onb) = onboarding {
        config::build_provider_from_onboarding(onb)?
    } else {
        config::build_provider(options, config)?
    };

    match provider {
        ResolvedProvider::DeepSeek(p) => {
            let provider = Arc::new(p);
            crate::runtime::register_cli_subagent_tool(
                &mut runtime.tools,
                &runtime.agent_config,
                provider.clone(),
            )?;
            crate::runtime::rebuild_prompt_assembly(&mut runtime);
            let mut session = AgentSession::new(runtime.agent_config)
                .context("failed to create agent session")?;
            run_with_provider(
                &mut session,
                provider.as_ref(),
                &runtime.tools,
                prompt,
                runtime.memory_store.clone(),
                config.billing.as_ref(),
            )
            .await?;
        }
        ResolvedProvider::Routed(p) => {
            let provider = Arc::new(p);
            crate::runtime::register_cli_subagent_tool(
                &mut runtime.tools,
                &runtime.agent_config,
                provider.clone(),
            )?;
            crate::runtime::rebuild_prompt_assembly(&mut runtime);
            let mut session = AgentSession::new(runtime.agent_config)
                .context("failed to create agent session")?;
            run_with_provider(
                &mut session,
                provider.as_ref(),
                &runtime.tools,
                prompt,
                runtime.memory_store.clone(),
                config.billing.as_ref(),
            )
            .await?;
        }
        ResolvedProvider::Mock(_) => {
            eprintln!("Note: using mock provider; no real model call is made.");
            let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
                message: Message::assistant("Mock provider has no real response configured."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            }]));
            crate::runtime::register_cli_subagent_tool(
                &mut runtime.tools,
                &runtime.agent_config,
                provider.clone(),
            )?;
            crate::runtime::rebuild_prompt_assembly(&mut runtime);
            let mut session = AgentSession::new(runtime.agent_config)
                .context("failed to create agent session")?;
            run_with_provider(
                &mut session,
                provider.as_ref(),
                &runtime.tools,
                prompt,
                runtime.memory_store.clone(),
                config.billing.as_ref(),
            )
            .await?;
        }
    }

    crate::runtime::process_diagnostics(&runtime.diagnostics, config).await;

    Ok(())
}

pub async fn run_chat(
    options: &SharedOptions,
    config: &FileConfig,
    onboarding: Option<crate::onboarding::OnboardingResult>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let mut runtime = crate::runtime::prepare_runtime(options, config, approval_handler)?;
    let provider = if let Some(ref onb) = onboarding {
        crate::build_erased_from_onboarding(onb)?
    } else {
        crate::build_erased_provider(options, config)?
    };
    crate::runtime::register_cli_subagent_tool(
        &mut runtime.tools,
        &runtime.agent_config,
        provider.clone(),
    )?;
    crate::runtime::rebuild_prompt_assembly(&mut runtime);

    let status = crate::context::build_status_text(
        options.model.as_deref(),
        runtime.project_root.as_deref(),
        &runtime.context,
    );
    eprintln!("{status}");
    eprintln!("Type prompts (Ctrl+D or /quit to exit).\n");

    let mut session = AgentSession::new(runtime.agent_config.clone())?;
    let tools = runtime.tools;
    let memory = runtime.memory_store;
    let billing = config.billing.clone();

    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        line.clear();
        eprint!("> ");
        let _ = io::stderr().flush();
        match stdin.read_line(&mut line) {
            Ok(0) => break, // Ctrl+D
            Ok(_) => {
                let prompt = line.trim().to_string();
                if prompt.is_empty() {
                    continue;
                }
                if prompt == "/quit" || prompt == "/exit" {
                    break;
                }
                tokio::select! {
                    result = run_interactive_turn(
                        &mut session,
                        provider.as_ref(),
                        &tools,
                        &memory,
                        billing.as_ref(),
                        prompt,
                    ) => {
                        if let Err(e) = result {
                            eprintln!("Error: {e}");
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        eprintln!("\nInterrupted");
                        continue;
                    }
                }
            }
            Err(e) => {
                eprintln!("Input error: {e}");
                break;
            }
        }
    }

    crate::runtime::process_diagnostics(&runtime.diagnostics, config).await;
    Ok(())
}

pub async fn run_tui(
    options: &SharedOptions,
    config: &FileConfig,
    onboarding: Option<crate::onboarding::OnboardingResult>,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<()> {
    let mut runtime = crate::runtime::prepare_runtime(options, config, approval_handler)?;
    let provider = if let Some(ref onb) = onboarding {
        crate::build_erased_from_onboarding(onb)?
    } else {
        crate::build_erased_provider(options, config)?
    };
    crate::runtime::register_cli_subagent_tool(
        &mut runtime.tools,
        &runtime.agent_config,
        provider.clone(),
    )?;
    crate::runtime::rebuild_prompt_assembly(&mut runtime);

    let status = crate::context::build_status_text(
        options.model.as_deref(),
        runtime.project_root.as_deref(),
        &runtime.context,
    );
    let auto_mode = config.auto_mode.unwrap_or(false);
    let project_root_or_cwd = runtime.project_root.clone().unwrap_or_else(|| {
        options.cwd.clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
    });

    let result = crate::tui::run(
        runtime.agent_config,
        provider,
        runtime.tools,
        status,
        runtime.project_root.as_deref(),
        &project_root_or_cwd,
        auto_mode,
        runtime.memory_store,
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
        config.billing.clone(),
    )
    .await;

    crate::runtime::process_diagnostics(&runtime.diagnostics, config).await;
    result
}

async fn run_interactive_turn(
    session: &mut AgentSession,
    provider: &dyn telos_agent::ModelProvider,
    tools: &ToolRegistry,
    memory_store: &Arc<Mutex<MemoryStore>>,
    billing: Option<&crate::config::BillingSection>,
    prompt: String,
) -> Result<()> {
    crate::memory_runtime::record_user_preference(memory_store, &prompt).await;
    let erased = telos_agent::ErasedProvider(provider);
    let mut stream = pin!(session.run_turn_stream(&erased, tools, prompt));
    let mut printed = String::new();
    let mut tool_details: HashMap<String, String> = HashMap::new();
    let cost_calculator = CostCalculator::from_section(billing);
    let mut total_cost = 0.0;
    while let Some(event) = stream.next().await {
        match event {
            Ok(telos_agent::TurnEvent::AssistantDelta { text }) => {
                print!("{text}");
                let _ = io::stdout().flush();
                printed.push_str(&text);
            }
            Ok(telos_agent::TurnEvent::ThinkingDelta { text }) => {
                eprintln!("\n[thinking] {text}");
            }
            Ok(telos_agent::TurnEvent::ToolCall { tool_call_id, name, detail }) => {
                tool_details.insert(tool_call_id, detail);
                eprintln!("\n[tool: {name}]");
            }
            Ok(telos_agent::TurnEvent::ToolCompleted { tool_call_id, name, is_error, .. }) => {
                if is_error {
                    eprintln!("[tool {name} failed]");
                } else {
                    eprintln!("[tool {name} completed]");
                    crate::memory_runtime::record_successful_tool(
                        memory_store,
                        &name,
                        &tool_call_id,
                        tool_details.get(&tool_call_id).map(String::as_str),
                    )
                    .await;
                }
            }
            Ok(telos_agent::TurnEvent::ToolResult(message)) => {
                for result in message.tool_results_iter() {
                    crate::memory_runtime::record_subagent_learning(memory_store, result).await;
                    if result.is_error {
                        crate::memory_runtime::record_tool_error(
                            memory_store,
                            result,
                            tool_details.get(&result.tool_call_id).map(String::as_str),
                        )
                        .await;
                    }
                }
            }
            Ok(telos_agent::TurnEvent::ProviderUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                prompt_cache_hit_tokens,
                prompt_cache_miss_tokens,
                reasoning_tokens,
                model,
            }) => {
                let usage = TokenUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    prompt_cache_hit_tokens,
                    prompt_cache_miss_tokens,
                    reasoning_tokens,
                };
                if let Some(estimate) = cost_calculator.estimate(model.as_deref(), &usage) {
                    total_cost += estimate.total;
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

    if total_cost > 0.0 {
        eprintln!("Estimated cost: {}", CostCalculator::format_cost(total_cost));
    }
    Ok(())
}

async fn run_with_provider(
    session: &mut AgentSession,
    provider: &dyn telos_agent::ModelProvider,
    tools: &ToolRegistry,
    prompt: String,
    memory_store: Arc<Mutex<MemoryStore>>,
    billing: Option<&crate::config::BillingSection>,
) -> Result<()> {
    crate::memory_runtime::record_user_preference(&memory_store, &prompt).await;
    let erased = telos_agent::ErasedProvider(provider);
    let mut stream = pin!(session.run_turn_stream(&erased, tools, prompt));
    let mut printed = String::new();
    let mut tool_details: HashMap<String, String> = HashMap::new();
    let cost_calculator = CostCalculator::from_section(billing);
    let mut total_cost = 0.0;
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
            Ok(telos_agent::TurnEvent::ToolCompleted { tool_call_id, name, is_error, .. }) => {
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
                    crate::memory_runtime::record_subagent_learning(&memory_store, result).await;
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
            Ok(telos_agent::TurnEvent::ProviderUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                prompt_cache_hit_tokens,
                prompt_cache_miss_tokens,
                reasoning_tokens,
                model,
            }) => {
                let usage = TokenUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    prompt_cache_hit_tokens,
                    prompt_cache_miss_tokens,
                    reasoning_tokens,
                };
                if let Some(estimate) = cost_calculator.estimate(model.as_deref(), &usage) {
                    total_cost += estimate.total;
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

    if total_cost > 0.0 {
        eprintln!("Estimated cost: {}", CostCalculator::format_cost(total_cost));
    }
    Ok(())
}
