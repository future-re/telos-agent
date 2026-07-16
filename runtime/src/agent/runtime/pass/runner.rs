use std::sync::Arc;

use tracing::{debug, info, info_span};

use crate::agent::context::Conversation;
use crate::agent::hooks::{HookContext, HookPhase};
use crate::agent::turn::{TurnEvent, TurnInputReceiver, TurnResult};
use crate::error::AgentError;
use crate::model::message::{Message, ToolCall};
use crate::model::provider::{ModelProvider, StopReason, TokenUsage};
use crate::tools::api::{ToolDefinition, ToolRegistry};

use super::super::{session::SessionInfo, state::RuntimeState};
use super::compaction::{self, CompactionPhaseResult};
use super::util;
use super::{Pass, PassControl, TurnState, hooks, injection, provider, tools};

pub(crate) async fn run_turn<P>(
    session: &mut SessionInfo,
    context: &mut Conversation,
    state: &mut RuntimeState,
    provider: &P,
    tools: &ToolRegistry,
    user_input: impl Into<String>,
    mut turn_input: TurnInputReceiver,
) -> Result<TurnResult, AgentError>
where
    P: ModelProvider,
{
    let mut tools = tools.clone();
    if let Some(skill_registry) = session.config().skill_registry.clone() {
        crate::tools::register_skill_tool(&mut tools, skill_registry);
    }

    let turn_id = session.advance_turn_id();
    let user_input = user_input.into();
    let mut turn = TurnState::new(turn_id);
    let mut events = Vec::new();
    let mut pass = Pass::BeginTurn;
    let mut sampled_message: Option<Message> = None;
    let mut sampled_stop_reason = StopReason::EndTurn;
    let mut tool_definitions: Vec<ToolDefinition> = Vec::new();
    let mut pending_tool_calls: Vec<ToolCall> = Vec::new();
    let mut save_error = None;

    loop {
        turn.enter(pass);
        let control = match pass {
            Pass::BeginTurn => {
                context.set_turn_memory_injected(false);
                context.set_turn_memory_mutation_notified(false);
                if session.config().prompt_assembly.is_none()
                    && session.config().base_system_prompt.is_none()
                {
                    session.config_mut().prompt_assembly =
                        Some(Arc::new(crate::agent::prompt::default_coding_assembly_for_profile(
                            Arc::new(tools.clone()),
                            session.config().cwd.clone(),
                            session.config().skill_registry.clone(),
                            session.config().path,
                            session.config().prompt_profile,
                        )));
                }
                context.repair_incomplete_tool_call_tail();
                let user_message = Message::user(user_input.clone());
                context.push_message(user_message.clone());
                emit(
                    session,
                    &mut events,
                    TurnEvent::TurnStarted {
                        session_id: session.session_id().to_string(),
                        turn_id,
                        user_input: user_input.clone(),
                    },
                );
                emit(session, &mut events, TurnEvent::User(user_message));
                state.metrics_mut().add_turn();
                info!(session_id = %session.session_id(), turn_id, "turn started");

                if context.cached_system_prompt().is_none()
                    && let Some(assembly) = &session.config().prompt_assembly
                {
                    context.set_cached_system_prompt(Some(assembly.build_blocks().await));
                }
                PassControl::Next(Pass::BeginIteration)
            }
            Pass::BeginIteration => {
                let iteration = turn.begin_iteration(session.config().max_iterations)?;
                state.metrics_mut().add_iteration();
                if session.config().cancellation.is_cancelled() {
                    return Err(AgentError::Cancelled);
                }
                tool_definitions = tools.definitions();
                let _guard =
                    info_span!("iteration", iteration, messages = context.messages().len())
                        .entered();
                debug!("iteration started");
                emit(
                    session,
                    &mut events,
                    TurnEvent::IterationStarted {
                        iteration,
                        message_count: context.messages().len(),
                    },
                );
                emit(
                    session,
                    &mut events,
                    TurnEvent::ProviderRequest {
                        iteration,
                        message_count: context.messages().len(),
                        tool_count: tool_definitions.len(),
                    },
                );
                PassControl::Next(Pass::DrainInput)
            }
            Pass::DrainInput => {
                let mut received = util::drain_turn_input(&mut turn_input);
                received.extend(util::drain_external_events(session));
                if !received.is_empty() {
                    turn.request_thinking();
                }
                for message in received {
                    context.push_message(message.clone());
                    emit(session, &mut events, TurnEvent::User(message));
                }
                PassControl::Next(Pass::CompactContext)
            }
            Pass::CompactContext => {
                match compaction::run_compaction_phase(
                    session,
                    context,
                    state,
                    provider,
                    turn.iteration(),
                )
                .await?
                {
                    CompactionPhaseResult::Continue { events: phase_events, compactions } => {
                        events.extend(phase_events);
                        for _ in 0..compactions {
                            state.metrics_mut().add_compaction();
                        }
                        PassControl::Next(Pass::InjectContext)
                    }
                    CompactionPhaseResult::AbortTurn { events: phase_events } => {
                        events.extend(phase_events);
                        sampled_message = Some(Message::assistant(""));
                        sampled_stop_reason = StopReason::EndTurn;
                        PassControl::Next(Pass::PersistTurn)
                    }
                }
            }
            Pass::InjectContext => {
                injection::inject_memory(session, context, &user_input, turn_id, turn.iteration());
                injection::inject_skill(session, context, &user_input, turn_id, turn.iteration());
                PassControl::Next(Pass::CallProvider)
            }
            Pass::CallProvider => {
                let system_prompt_blocks = if let Some(blocks) = context.cached_system_prompt() {
                    blocks.clone()
                } else if let Some(system_prompt) = &session.config().base_system_prompt {
                    vec![crate::agent::prompt::PromptBlock::dynamic(
                        "base_system_prompt",
                        system_prompt,
                    )]
                } else {
                    Vec::new()
                };
                let hint = turn.model_hint(session.config());
                let (message, reason, usage, model, provider_events) = provider::call_with_retry(
                    session,
                    context,
                    state,
                    provider,
                    &system_prompt_blocks,
                    &tool_definitions,
                    hint,
                )
                .await?;
                events.extend(provider_events);
                record_usage(session, state, &mut events, usage, model);
                context.push_message(message.clone());
                emit(session, &mut events, TurnEvent::Assistant(message.clone()));
                sampled_stop_reason = reason;
                sampled_message = Some(message);
                PassControl::Next(Pass::PostSamplingHooks)
            }
            Pass::PostSamplingHooks => {
                let message = sampled_message.as_ref().expect("provider pass sets message");
                let hook_context = HookContext {
                    session_id: session.session_id().to_string(),
                    turn_id,
                    message_count: context.messages().len(),
                };
                events.extend(
                    hooks::run_hook_phase(
                        session,
                        context,
                        HookPhase::PostSampling,
                        &hook_context,
                        message,
                    )
                    .await?,
                );
                PassControl::Next(Pass::RouteAssistant)
            }
            Pass::RouteAssistant => {
                let message = sampled_message.as_ref().expect("provider pass sets message");
                turn.observe_assistant(message);
                pending_tool_calls = message.tool_calls().cloned().collect();
                if !pending_tool_calls.is_empty() {
                    PassControl::Next(Pass::ExecuteTools)
                } else {
                    let mut received = util::drain_turn_input(&mut turn_input);
                    received.extend(util::drain_external_events(session));
                    if received.is_empty() {
                        PassControl::Next(Pass::StopHooks)
                    } else {
                        for input in received {
                            context.push_message(input.clone());
                            emit(session, &mut events, TurnEvent::User(input));
                        }
                        turn.request_thinking();
                        PassControl::Next(Pass::BeginIteration)
                    }
                }
            }
            Pass::ExecuteTools => {
                if session.config().cancellation.is_cancelled() {
                    return Err(AgentError::Cancelled);
                }
                let (tool_message, tool_events) = tools::execute_tool_calls_phase(
                    session,
                    context,
                    state,
                    &tools,
                    std::mem::take(&mut pending_tool_calls),
                    turn_id,
                )
                .await?;
                events.extend(tool_events);
                turn.observe_tool_results(&tool_message);
                context.push_message(tool_message.clone());
                emit(session, &mut events, TurnEvent::ToolResult(tool_message));
                PassControl::Next(Pass::BeginIteration)
            }
            Pass::StopHooks => {
                let message = sampled_message.as_ref().expect("provider pass sets message");
                let hook_context = HookContext {
                    session_id: session.session_id().to_string(),
                    turn_id,
                    message_count: context.messages().len(),
                };
                events.extend(
                    hooks::run_hook_phase(
                        session,
                        context,
                        HookPhase::Stop,
                        &hook_context,
                        message,
                    )
                    .await?,
                );
                PassControl::Next(Pass::PersistTurn)
            }
            Pass::PersistTurn => {
                save_error = super::super::session::persistence::save(
                    session.session_id(),
                    session.config(),
                    context.messages(),
                    state.metrics(),
                    state.read_file_state(),
                    session.next_turn_id(),
                )
                .await
                .err();
                if let Some(error) = &save_error {
                    emit(
                        session,
                        &mut events,
                        TurnEvent::PersistenceFailed { error: error.to_string() },
                    );
                }
                PassControl::Next(Pass::FinishTurn)
            }
            Pass::FinishTurn => {
                let final_message =
                    sampled_message.as_ref().cloned().unwrap_or_else(|| Message::assistant(""));
                emit(
                    session,
                    &mut events,
                    TurnEvent::TurnFinished {
                        stop_reason: sampled_stop_reason,
                        final_text: final_message.text_content(),
                    },
                );
                info!(stop_reason = ?sampled_stop_reason, "turn finished");
                PassControl::Complete
            }
        };

        match control {
            PassControl::Next(next) => pass = next,
            PassControl::Complete => break,
        }
    }

    Ok(TurnResult {
        events,
        final_message: sampled_message.unwrap_or_else(|| Message::assistant("")),
        stop_reason: sampled_stop_reason,
        save_error,
    })
}

fn emit(session: &SessionInfo, events: &mut Vec<TurnEvent>, event: TurnEvent) {
    util::broadcast(session, &event);
    events.push(event);
}

fn record_usage(
    session: &SessionInfo,
    state: &mut RuntimeState,
    events: &mut Vec<TurnEvent>,
    usage: Option<TokenUsage>,
    model: Option<String>,
) {
    let Some(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        prompt_cache_hit_tokens,
        prompt_cache_miss_tokens,
        reasoning_tokens,
    }) = usage
    else {
        return;
    };
    state.metrics_mut().add_input_tokens(input_tokens);
    state.metrics_mut().add_output_tokens(output_tokens);
    if let Some(tokens) = prompt_cache_hit_tokens {
        state.metrics_mut().add_prompt_cache_hit_tokens(tokens);
    }
    if let Some(tokens) = prompt_cache_miss_tokens {
        state.metrics_mut().add_prompt_cache_miss_tokens(tokens);
    }
    emit(
        session,
        events,
        TurnEvent::ProviderUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            prompt_cache_hit_tokens,
            prompt_cache_miss_tokens,
            reasoning_tokens,
            model,
        },
    );
}
