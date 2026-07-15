use std::sync::Arc;
use tracing::{debug, error, info, info_span, warn};

use crate::context::ContextOps;
use crate::error::AgentError;
use crate::hooks::{HookContext, HookPhase};
use crate::message::Message;
use crate::provider::{ModelHint, ModelProvider, StopReason, TokenUsage};
use crate::session::SessionOps;
use crate::state::StateOps;
use crate::tool::ToolRegistry;
use crate::turn::{TurnEvent, TurnResult};

use super::compaction_phase;
use super::compaction_phase::CompactionPhaseResult;
use super::hook_phase;
use super::injection_phase;
use super::tool_phase;
use super::util;

/// Run one turn to completion and return the collected events.
///
/// This is the **non-streaming** entry point: it runs the full turn loop
/// internally, collects all [`TurnEvent`]s, and returns a [`TurnResult`].
pub async fn run_turn<S, C, St, P>(
    session: &mut S,
    context: &mut C,
    state: &mut St,
    provider: &P,
    tools: &ToolRegistry,
    user_input: impl Into<String>,
) -> Result<TurnResult, AgentError>
where
    S: SessionOps,
    C: ContextOps,
    St: StateOps,
    P: ModelProvider,
{
    let messages_before = context.messages().to_vec();
    let _turn_id_before = session.next_turn_id();
    let metrics_checkpoint = state.metrics().checkpoint();
    let read_file_state_before = state.read_file_state_owned();

    let mut events = Vec::new();
    let mut final_message: Option<Message> = None;
    let mut in_hook_phase = false;

    let turn_result: Result<StopReason, AgentError> = async {
        let mut tools = tools.clone();
        if let Some(skill_registry) = session.config().skill_registry.clone() {
            crate::tools::register_skill_tool(&mut tools, skill_registry);
        }
        let tools = tools;

        let turn_id = session.advance_turn_id();
        let user_input = user_input.into();
        context.set_turn_memory_injected(false);
        context.set_turn_memory_mutation_notified(false);

        if session.config().prompt_assembly.is_none()
            && session.config().base_system_prompt.is_none()
        {
            session.config_mut().prompt_assembly =
                Some(Arc::new(crate::prompt::default_coding_assembly_for_profile(
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

        let user_input_for_memory = user_input.clone();

        let started = TurnEvent::TurnStarted {
            session_id: session.session_id().to_string(),
            turn_id,
            user_input: user_input.clone(),
        };
        util::broadcast(session, &started);
        events.push(started);
        events.push(TurnEvent::User(user_message));

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

        let stop_reason = loop {
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
                let _guard = info_span!(
                    "iteration",
                    iteration = iterations,
                    messages = context.messages().len()
                )
                .entered();
                debug!("iteration started");
            }

            let iteration_started = TurnEvent::IterationStarted {
                iteration: iterations,
                message_count: context.messages().len(),
            };
            util::broadcast(session, &iteration_started);
            events.push(iteration_started);

            let provider_request = TurnEvent::ProviderRequest {
                iteration: iterations,
                message_count: context.messages().len(),
                tool_count: tool_definitions.len(),
            };
            util::broadcast(session, &provider_request);
            events.push(provider_request);

            match compaction_phase::run_compaction_phase(
                session, context, state, provider, iterations,
            )
            .await?
            {
                CompactionPhaseResult::Continue { events: phase_events, compactions } => {
                    for event in phase_events {
                        util::broadcast(session, &event);
                        events.push(event);
                    }
                    for _ in 0..compactions {
                        state.metrics_mut().add_compaction();
                    }
                }
                CompactionPhaseResult::AbortTurn { events: phase_events } => {
                    for event in phase_events {
                        util::broadcast(session, &event);
                        events.push(event);
                    }
                    let aborted = TurnEvent::TurnFinished {
                        stop_reason: StopReason::EndTurn,
                        final_text: String::new(),
                    };
                    util::broadcast(session, &aborted);
                    events.push(aborted);
                    break StopReason::EndTurn;
                }
            }

            for ext_msg in util::drain_external_events(session) {
                context.push_message(ext_msg.clone());
                events.push(TurnEvent::User(ext_msg));
            }

            injection_phase::inject_memory(
                session,
                context,
                &user_input_for_memory,
                turn_id,
                iterations,
            );
            injection_phase::inject_skill(
                session,
                context,
                &user_input_for_memory,
                turn_id,
                iterations,
            );

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

            let (assistant_message, reason, usage, actual_model, provider_events) =
                provider_retry::call_with_retry(
                    session,
                    context,
                    state,
                    provider,
                    &system_prompt_blocks,
                    &tool_definitions,
                    hint,
                )
                .await?;
            let stop_reason = reason;

            for event in &provider_events {
                util::broadcast(session, event);
                events.push(event.clone());
            }

            if let Some(TokenUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                prompt_cache_hit_tokens,
                prompt_cache_miss_tokens,
                reasoning_tokens,
            }) = usage
            {
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
                events.push(usage_event);
            }

            context.push_message(assistant_message.clone());
            let assistant_event = TurnEvent::Assistant(assistant_message.clone());
            util::broadcast(session, &assistant_event);
            events.push(assistant_event);

            let hook_context = HookContext {
                session_id: session.session_id().to_string(),
                turn_id,
                message_count: context.messages().len(),
            };

            let post_events = hook_phase::run_hook_phase(
                session,
                context,
                HookPhase::PostSampling,
                &hook_context,
                &assistant_message,
            )
            .await?;
            for event in post_events {
                util::broadcast(session, &event);
                events.push(event);
            }

            let tool_calls = assistant_message.tool_calls().cloned().collect::<Vec<_>>();

            if !tool_calls.is_empty() && assistant_message.text_content().is_empty() {
                consecutive_noop += 1;
            } else if !tool_calls.is_empty() {
                consecutive_noop = 0;
            }

            if tool_calls.is_empty() {
                let reconsideration_inputs = util::drain_external_events(session);
                if !reconsideration_inputs.is_empty() {
                    for message in reconsideration_inputs {
                        let user_event = TurnEvent::User(message);
                        util::broadcast(session, &user_event);
                        events.push(user_event);
                    }
                    force_thinking_next_iteration = true;
                    continue;
                }

                let stop_events = hook_phase::run_hook_phase(
                    session,
                    context,
                    HookPhase::Stop,
                    &hook_context,
                    &assistant_message,
                )
                .await?;
                for event in stop_events {
                    util::broadcast(session, &event);
                    events.push(event);
                }

                let finished = TurnEvent::TurnFinished {
                    stop_reason,
                    final_text: assistant_message.text_content(),
                };
                util::broadcast(session, &finished);
                events.push(finished);
                info!(?stop_reason, "turn finished");
                break stop_reason;
            }

            if session.config().cancellation.is_cancelled() {
                Err(AgentError::Cancelled)?;
            }

            let (tool_message, tool_events) = tool_phase::execute_tool_calls_phase(
                session, context, state, &tools, tool_calls, turn_id,
            )
            .await?;
            for event in tool_events {
                util::broadcast(session, &event);
                events.push(event);
            }

            previous_tool_error = tool_message.tool_results_iter().any(|r| r.is_error);

            context.push_message(tool_message.clone());
            let result_event = TurnEvent::ToolResult(tool_message);
            util::broadcast(session, &result_event);
            events.push(result_event);

            for ext_msg in util::drain_external_events(session) {
                context.push_message(ext_msg.clone());
                events.push(TurnEvent::User(ext_msg));
            }
        };

        Ok(stop_reason)
    }
    .await;

    let stop_reason = match turn_result {
        Ok(reason) => reason,
        Err(err) => {
            *context.messages_mut() = messages_before;
            session.config_mut();
            state.metrics().restore(&metrics_checkpoint);
            state.set_read_file_state(read_file_state_before);
            return Err(err);
        }
    };

    // Derive final_message from collected events
    for event in &events {
        match event {
            TurnEvent::HookStarted { .. } => in_hook_phase = true,
            TurnEvent::HookCompleted { .. } => in_hook_phase = false,
            TurnEvent::IterationStarted { .. } => in_hook_phase = false,
            TurnEvent::Assistant(message) if !in_hook_phase => {
                final_message = Some(message.clone());
            }
            _ => {}
        }
    }

    let save_error = match crate::session::persistence::save(
        session.session_id(),
        session.config(),
        context.messages(),
        state.metrics(),
        state.read_file_state(),
    )
    .await
    {
        Ok(()) => None,
        Err(err) => {
            error!(error = %err, "failed to persist session after turn");
            Some(err)
        }
    };

    Ok(TurnResult {
        final_message: final_message.unwrap_or_else(|| Message::assistant("")),
        events,
        stop_reason,
        save_error,
    })
}
