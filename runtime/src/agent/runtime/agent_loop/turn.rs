use std::sync::Arc;

use tracing::{debug, info};

use crate::agent::context::Conversation;
use crate::agent::policies::{PolicyContext, PolicyDecision};
use crate::agent::turn::{TurnEvent, TurnInputReceiver};
use crate::error::AgentError;
use crate::model::message::{Message, ToolCall};
use crate::model::provider::{ModelProvider, TokenUsage};
use crate::tools::api::ToolRegistry;

use super::super::{session::SessionInfo, state::RuntimeState};
use super::TurnOutcome;
use super::compaction;
use super::input;
use super::{injection, provider, state::LoopState, tools};

pub(crate) async fn run_turn<P>(
    session: &mut SessionInfo,
    context: &mut Conversation,
    state: &mut RuntimeState,
    provider: &P,
    tools: &ToolRegistry,
    user_input: impl Into<String>,
    mut turn_input: TurnInputReceiver,
) -> Result<TurnOutcome, AgentError>
where
    P: ModelProvider,
{
    let mut tools = tools.clone();
    if let Some(skill_registry) = session.config().skill_registry.clone() {
        crate::tools::register_skill_tool(&mut tools, skill_registry);
    }

    let turn_id = session.advance_turn_id();
    let user_input = user_input.into();
    let mut loop_state = LoopState::new();
    begin_turn(session, context, state, &tools, &user_input, turn_id).await?;

    let (final_message, stop_reason) = loop {
        let iteration = loop_state.begin_iteration(session.config().max_iterations)?;
        state.metrics_mut().add_iteration();
        if session.config().cancellation.is_cancelled() {
            return Err(AgentError::Cancelled);
        }

        debug!(iteration, message_count = context.messages().len(), "iteration started");
        emit(
            session,
            TurnEvent::IterationStarted { iteration, message_count: context.messages().len() },
        );

        append_pending_inputs(session, context, &mut turn_input, &mut loop_state)?;

        if compaction::compact_if_needed(session, context, state, provider, iteration).await? {
            state.metrics_mut().add_compaction();
        }

        injection::inject_memory(session, context, &user_input, turn_id, iteration);
        injection::inject_skill(session, context, &user_input, turn_id, iteration);

        let system_prompt_blocks = if let Some(blocks) = context.cached_system_prompt() {
            blocks.clone()
        } else if let Some(system_prompt) = &session.config().base_system_prompt {
            vec![crate::agent::prompt::PromptBlock::dynamic("base_system_prompt", system_prompt)]
        } else {
            Vec::new()
        };
        let tool_definitions = tools.definitions();
        let hint = loop_state.model_hint(session.config());
        loop_state.queue_feedback(
            run_policies(
                session,
                "model_before_request",
                session.config().policies.model_before_request(),
                PolicyContext::ModelBeforeRequest {
                    session_id: session.session_id().to_string(),
                    turn_id,
                    iteration,
                    message_count: context.messages().len(),
                    system_prompt_block_count: system_prompt_blocks.len(),
                    tool_names: tool_definitions.iter().map(|tool| tool.name.clone()).collect(),
                    model_hint: format!("{hint:?}").to_lowercase(),
                },
            )
            .await?,
        );
        if append_feedback(session, context, &mut loop_state)? {
            continue;
        }
        emit(
            session,
            TurnEvent::ProviderRequest {
                iteration,
                message_count: context.messages().len(),
                tool_count: tool_definitions.len(),
            },
        );
        let response = provider::complete_with_retry(
            session,
            context,
            state,
            provider,
            &system_prompt_blocks,
            &tool_definitions,
            hint,
        )
        .await?;
        record_usage(session, state, response.usage, response.model);
        let message = response.message;
        let reason = response.stop_reason;
        context.journal().append_assistant(message.clone())?;
        emit(session, TurnEvent::Assistant(message.clone()));

        loop_state.queue_feedback(
            run_policies(
                session,
                "model_response",
                session.config().policies.model_response(),
                PolicyContext::ModelResponse {
                    session_id: session.session_id().to_string(),
                    turn_id,
                    iteration,
                    message: message.clone(),
                },
            )
            .await?,
        );

        loop_state.observe_assistant(&message);
        let pending_tool_calls: Vec<ToolCall> = message.tool_calls().cloned().collect();
        if !pending_tool_calls.is_empty() {
            if session.config().cancellation.is_cancelled() {
                return Err(AgentError::Cancelled);
            }
            let tools::ToolBatchOutcome { message: tool_message, feedback } =
                tools::execute(session, context, state, &tools, pending_tool_calls, turn_id)
                    .await?;
            loop_state.observe_tool_results(&tool_message);
            context.journal().resolve_tool_calls(tool_message.clone())?;
            emit(session, TurnEvent::ToolResult(tool_message));
            loop_state.queue_feedback(feedback);
            append_feedback(session, context, &mut loop_state)?;

            // Tool results are model input, never a terminal assistant response.
            continue;
        }

        if append_pending_inputs(session, context, &mut turn_input, &mut loop_state)? {
            continue;
        }

        loop_state.queue_feedback(
            run_policies(
                session,
                "turn_before_finish",
                session.config().policies.turn_before_finish(),
                PolicyContext::TurnBeforeFinish {
                    session_id: session.session_id().to_string(),
                    turn_id,
                    message: message.clone(),
                },
            )
            .await?,
        );
        if append_feedback(session, context, &mut loop_state)? {
            continue;
        }
        break (message, reason);
    };

    super::super::session::persistence::save_with_events(
        session,
        context.messages(),
        state.metrics(),
        state.read_file_state(),
        session.next_turn_id(),
        "turn_finish",
    )
    .await?;

    emit(
        session,
        TurnEvent::TurnFinished { stop_reason, final_text: final_message.text_content() },
    );
    info!(stop_reason = ?stop_reason, "turn finished");

    Ok(TurnOutcome { final_message, stop_reason })
}

async fn begin_turn(
    session: &mut SessionInfo,
    context: &mut Conversation,
    state: &mut RuntimeState,
    tools: &ToolRegistry,
    user_input: &str,
    turn_id: u64,
) -> Result<(), AgentError> {
    context.set_turn_memory_injected(false);
    context.set_turn_memory_mutation_notified(false);
    if session.config().prompt_assembly.is_none() && session.config().base_system_prompt.is_none() {
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
    emit(
        session,
        TurnEvent::TurnStarted {
            session_id: session.session_id().to_string(),
            turn_id,
            user_input: user_input.to_string(),
        },
    );
    let feedback = run_policies(
        session,
        "turn_start",
        session.config().policies.turn_start(),
        PolicyContext::TurnStart {
            session_id: session.session_id().to_string(),
            turn_id,
            input: user_input.to_string(),
        },
    )
    .await?;
    let user_message = Message::user(user_input);
    context.journal().append_user(user_message.clone())?;
    emit(session, TurnEvent::User(user_message));
    if !feedback.is_empty() {
        let feedback_message = Message::user(feedback.join("\n\n"));
        context.journal().append_user(feedback_message.clone())?;
        emit(session, TurnEvent::User(feedback_message));
    }
    state.metrics_mut().add_turn();
    info!(session_id = %session.session_id(), turn_id, "turn started");

    if context.cached_system_prompt().is_none()
        && let Some(assembly) = &session.config().prompt_assembly
    {
        context.set_cached_system_prompt(Some(assembly.build_blocks().await));
    }
    Ok(())
}

fn append_pending_inputs(
    session: &mut SessionInfo,
    context: &mut Conversation,
    turn_input: &mut TurnInputReceiver,
    loop_state: &mut LoopState,
) -> Result<bool, AgentError> {
    let received = input::drain_pending(session, turn_input);
    if !received.is_empty() {
        loop_state.request_thinking();
    }
    let appended = !received.is_empty();
    for message in received {
        context.journal().append_user(message.clone())?;
        emit(session, TurnEvent::User(message));
    }
    Ok(appended)
}

fn append_feedback(
    session: &SessionInfo,
    context: &mut Conversation,
    loop_state: &mut LoopState,
) -> Result<bool, AgentError> {
    let feedback = loop_state.take_feedback();
    if feedback.is_empty() {
        return Ok(false);
    }
    let message = Message::user(feedback.join("\n\n"));
    context.journal().append_user(message.clone())?;
    emit(session, TurnEvent::User(message));
    loop_state.request_thinking();
    Ok(true)
}

async fn run_policies(
    session: &SessionInfo,
    point: &str,
    policies: Vec<Arc<dyn crate::Policy>>,
    context: PolicyContext,
) -> Result<Vec<String>, AgentError> {
    let mut feedback = Vec::new();
    for policy in policies {
        let name = policy.name().to_string();
        emit(session, TurnEvent::PolicyStarted { point: point.into(), name: name.clone() });
        let outcome = match policy.evaluate(&context).await {
            Ok(outcome) => outcome,
            Err(error) => {
                emit(
                    session,
                    TurnEvent::PolicyFailed { point: point.into(), name, error: error.to_string() },
                );
                return Err(error);
            }
        };
        let feedback_count = outcome.feedback.len();
        feedback.extend(outcome.feedback);
        if let PolicyDecision::Reject { reason } = outcome.decision {
            emit(
                session,
                TurnEvent::PolicyRejected {
                    point: point.into(),
                    name: name.clone(),
                    reason: reason.clone(),
                },
            );
            return Err(AgentError::PermissionDenied(format!(
                "policy `{}` rejected: {reason}",
                name
            )));
        }
        emit(session, TurnEvent::PolicyCompleted { point: point.into(), name, feedback_count });
    }
    Ok(feedback)
}

fn emit(session: &SessionInfo, event: TurnEvent) {
    session.emit_turn_event(&event);
}

fn record_usage(
    session: &SessionInfo,
    state: &mut RuntimeState,
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
