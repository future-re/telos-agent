use async_stream::try_stream;
use futures_core::stream::Stream;
use std::sync::Arc;
use tracing::{debug, info, info_span, warn};

use crate::context::ContextOps;
use crate::error::AgentError;
use crate::hooks::{HookContext, HookPhase};
use crate::message::Message;
use crate::provider::{ModelHint, ModelProvider, StopReason, TokenUsage};
use crate::session::SessionOps;
use crate::state::StateOps;
use crate::tool::ToolRegistry;
use crate::turn::{TurnEvent, TurnInputReceiver, empty_turn_input_receiver};

use super::compaction_phase;
use super::compaction_phase::CompactionPhaseResult;
use super::hook_phase;
use super::injection_phase;
use super::tool_phase;
use super::util;

/// Run one turn, yielding TurnEvents as the turn progresses.
pub fn run_turn_stream<'a, S, C, St, P>(
    session: &'a mut S,
    context: &'a mut C,
    state: &'a mut St,
    provider: &'a P,
    tools: &'a ToolRegistry,
    user_input: impl Into<String> + 'a,
) -> impl Stream<Item = Result<TurnEvent, AgentError>> + 'a
where
    S: SessionOps + 'a,
    C: ContextOps + 'a,
    St: StateOps + 'a,
    P: ModelProvider + 'a,
{
    run_turn_stream_with_input(
        session,
        context,
        state,
        provider,
        tools,
        user_input,
        empty_turn_input_receiver(),
    )
}

/// Run one turn with a live input channel.
pub fn run_turn_stream_with_input<'a, S, C, St, P>(
    session: &'a mut S,
    context: &'a mut C,
    state: &'a mut St,
    provider: &'a P,
    tools: &'a ToolRegistry,
    user_input: impl Into<String> + 'a,
    mut turn_input: TurnInputReceiver,
) -> impl Stream<Item = Result<TurnEvent, AgentError>> + 'a
where
    S: SessionOps + 'a,
    C: ContextOps + 'a,
    St: StateOps + 'a,
    P: ModelProvider + 'a,
{
    try_stream! {
        let mut tools = tools.clone();
        if let Some(skill_registry) = session.config().skill_registry.clone() {
            crate::tools::register_skill_tool(&mut tools, skill_registry);
        }
        let tools = tools;

        let turn_id = session.advance_turn_id();
        let user_input = user_input.into();
        context.set_turn_memory_injected(false);
        context.set_turn_memory_mutation_notified(false);

        if session.config().prompt_assembly.is_none() && session.config().base_system_prompt.is_none() {
            session.config_mut().prompt_assembly = Some(Arc::new(
                crate::prompt::default_coding_assembly_for_profile(
                    Arc::new(tools.clone()),
                    session.config().cwd.clone(),
                    session.config().skill_registry.clone(),
                    session.config().path,
                    session.config().prompt_profile,
                ),
            ));
        }

        context.repair_incomplete_tool_call_tail();

        let user_message = Message::user(user_input.clone());
        context.push_message(user_message.clone());

        let user_input_for_memory = user_input.clone();

        let started = TurnEvent::TurnStarted {
            session_id: session.session_id().to_string(),
            turn_id,
            user_input: user_input.clone(),
        };
        util::broadcast(session, &started);
        yield started;
        yield TurnEvent::User(user_message);

        state.metrics_mut().add_turn();

        {
            let _guard = info_span!("turn", session_id = %session.session_id(), turn_id).entered();
            info!("turn started");
        }

        let mut iterations = 0;
        let mut previous_tool_error = false;
        let mut consecutive_noop = 0usize;
        let mut force_thinking_next_iteration = false;

        if context.cached_system_prompt().is_none()
            && let Some(assembly) = &session.config().prompt_assembly
        {
            let blocks = assembly.build_blocks().await;
            let section_stats = blocks
                .iter()
                .map(|block| format!("{}:{} chars", block.name, block.text.chars().count()))
                .collect::<Vec<_>>();
            let total_chars: usize = blocks.iter().map(|block| block.text.chars().count()).sum();
            info!(
                prompt_profile = ?session.config().prompt_profile,
                prompt_sections = blocks.len(),
                prompt_total_chars = total_chars,
                prompt_section_stats = ?section_stats,
                "built system prompt"
            );
            context.set_cached_system_prompt(Some(blocks));
        }

        loop {
            if let Some(max_iterations) = session.config().max_iterations
                && iterations >= max_iterations
            {
                Err(AgentError::MaxIterations(max_iterations))?;
            }
            iterations += 1;
            state.metrics_mut().add_iteration();

            if session.config().cancellation.is_cancelled() {
                warn!("turn cancelled during iteration {}", iterations);
                Err(AgentError::Cancelled)?;
            }

            let tool_definitions = tools.definitions();
            {
                let _guard = info_span!("iteration", iteration = iterations, messages = context.messages().len()).entered();
                debug!("iteration started");
            }

            let iteration_started = TurnEvent::IterationStarted {
                iteration: iterations,
                message_count: context.messages().len(),
            };
            util::broadcast(session, &iteration_started);
            yield iteration_started;

            let provider_request = TurnEvent::ProviderRequest {
                iteration: iterations,
                message_count: context.messages().len(),
                tool_count: tool_definitions.len(),
            };
            util::broadcast(session, &provider_request);
            yield provider_request;

            match compaction_phase::run_compaction_phase(session, context, state, provider, iterations).await? {
                CompactionPhaseResult::Continue { events, compactions } => {
                    for event in events {
                        util::broadcast(session, &event);
                        yield event;
                    }
                    for _ in 0..compactions {
                        state.metrics_mut().add_compaction();
                    }
                }
                CompactionPhaseResult::AbortTurn { events } => {
                    for event in events {
                        util::broadcast(session, &event);
                        yield event;
                    }
                    let aborted = TurnEvent::TurnFinished {
                        stop_reason: StopReason::EndTurn,
                        final_text: String::new(),
                    };
                    util::broadcast(session, &aborted);
                    yield aborted;
                    break;
                }
            }

            for message in util::drain_turn_input(&mut turn_input) {
                context.push_message(message.clone());
                force_thinking_next_iteration = true;
                let user_event = TurnEvent::User(message);
                util::broadcast(session, &user_event);
                yield user_event;
            }

            for ext_msg in util::drain_external_events(session) {
                context.push_message(ext_msg.clone());
                yield TurnEvent::User(ext_msg);
            }

            injection_phase::inject_memory(session, context, &user_input_for_memory, turn_id, iterations);
            injection_phase::inject_skill(session, context, &user_input_for_memory, turn_id, iterations);

            let hint = if force_thinking_next_iteration {
                force_thinking_next_iteration = false;
                ModelHint::Thinking
            } else {
                util::resolve_hint(
                    session.config(),
                    iterations,
                    previous_tool_error,
                    consecutive_noop,
                )
            };

            let system_prompt_blocks = if let Some(blocks) = context.cached_system_prompt() {
                blocks.clone()
            } else if let Some(system_prompt) = &session.config().base_system_prompt {
                vec![crate::prompt::PromptBlock::dynamic("base_system_prompt", system_prompt)]
            } else {
                Vec::new()
            };

            let (assistant_message, stop_reason, usage, actual_model, provider_events) =
                provider_retry::call_with_retry(
                    session, context, state, provider,
                    &system_prompt_blocks,
                    &tool_definitions,
                    hint,
                ).await?;

            for event in &provider_events {
                util::broadcast(session, event);
                yield event.clone();
            }

            if let Some(TokenUsage { input_tokens, output_tokens, total_tokens, prompt_cache_hit_tokens, prompt_cache_miss_tokens, reasoning_tokens }) = usage {
                state.metrics_mut().add_input_tokens(input_tokens);
                state.metrics_mut().add_output_tokens(output_tokens);
                if let Some(hit) = prompt_cache_hit_tokens {
                    state.metrics_mut().add_prompt_cache_hit_tokens(hit);
                }
                if let Some(miss) = prompt_cache_miss_tokens {
                    state.metrics_mut().add_prompt_cache_miss_tokens(miss);
                }
                debug!(
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    prompt_cache_hit_tokens,
                    prompt_cache_miss_tokens,
                    reasoning_tokens,
                    model = ?actual_model,
                    "provider usage"
                );
                let usage_event = TurnEvent::ProviderUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    prompt_cache_hit_tokens,
                    prompt_cache_miss_tokens,
                    reasoning_tokens,
                    model: actual_model,
                };
                util::broadcast(session, &usage_event);
                yield usage_event;
            }

            context.push_message(assistant_message.clone());
            let assistant_event = TurnEvent::Assistant(assistant_message.clone());
            util::broadcast(session, &assistant_event);
            yield assistant_event;

            let hook_context = HookContext {
                session_id: session.session_id().to_string(),
                turn_id,
                message_count: context.messages().len(),
            };

            let post_events = hook_phase::run_hook_phase(
                session, context,
                HookPhase::PostSampling, &hook_context, &assistant_message,
            ).await?;
            for event in post_events {
                util::broadcast(session, &event);
                yield event;
            }

            let tool_calls = assistant_message.tool_calls().cloned().collect::<Vec<_>>();

            if !tool_calls.is_empty() && assistant_message.text_content().is_empty() {
                consecutive_noop += 1;
            } else if !tool_calls.is_empty() {
                consecutive_noop = 0;
            }

            if tool_calls.is_empty() {
                let mut reconsideration_inputs = Vec::new();
                for message in util::drain_turn_input(&mut turn_input) {
                    context.push_message(message.clone());
                    reconsideration_inputs.push(message);
                }
                for ext_msg in util::drain_external_events(session) {
                    context.push_message(ext_msg.clone());
                    reconsideration_inputs.push(ext_msg);
                }
                if !reconsideration_inputs.is_empty() {
                    for message in reconsideration_inputs {
                        let user_event = TurnEvent::User(message);
                        util::broadcast(session, &user_event);
                        yield user_event;
                    }
                    force_thinking_next_iteration = true;
                    continue;
                }

                let stop_events = hook_phase::run_hook_phase(
                    session, context,
                    HookPhase::Stop, &hook_context, &assistant_message,
                ).await?;
                for event in stop_events {
                    util::broadcast(session, &event);
                    yield event;
                }

                let finished = TurnEvent::TurnFinished {
                    stop_reason,
                    final_text: assistant_message.text_content(),
                };
                util::broadcast(session, &finished);
                yield finished;
                info!(?stop_reason, "turn finished");
                break;
            }

            if session.config().cancellation.is_cancelled() {
                Err(AgentError::Cancelled)?;
            }

            let (tool_message, tool_events) =
                tool_phase::execute_tool_calls_phase(session, context, state, &tools, tool_calls, turn_id).await?;
            for event in tool_events {
                util::broadcast(session, &event);
                yield event;
            }

            previous_tool_error = tool_message.tool_results_iter().any(|r| r.is_error);

            context.push_message(tool_message.clone());
            let result_event = TurnEvent::ToolResult(tool_message);
            util::broadcast(session, &result_event);
            yield result_event;

            for message in util::drain_turn_input(&mut turn_input) {
                context.push_message(message.clone());
                force_thinking_next_iteration = true;
                let user_event = TurnEvent::User(message);
                util::broadcast(session, &user_event);
                yield user_event;
            }

            for ext_msg in util::drain_external_events(session) {
                context.push_message(ext_msg.clone());
                yield TurnEvent::User(ext_msg);
            }
        }
    }
}
