use futures_core::Stream;
use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::agent::context::Conversation;
use crate::agent::prompt::PromptBlock;
use crate::config::CancellationState;
use crate::error::AgentError;
use crate::model::message::{Message, Role};
use crate::model::provider::{
    CompletionRequest, CompletionResponse, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};
use crate::{ModelHint, ToolDefinition, TurnEvent};

use super::super::{session::SessionInfo, state::RuntimeState};

pub async fn call_provider<P: ModelProvider, F: FnMut(TurnEvent)>(
    request: CompletionRequest,
    provider: &P,
    cancellation: &CancellationState,
    mut emit: F,
) -> Result<CompletionResponse, AgentError> {
    let mut stream: std::pin::Pin<
        Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send>,
    > = Box::pin(provider.stream_complete(request));

    let mut text = String::new();
    let mut thinking = String::new();
    let mut tool_calls = Vec::new();

    let mut stop_reason = StopReason::EndTurn;
    let mut usage = None;
    let mut model = None;

    let mut message = Message { role: Role::Assistant, blocks: Vec::new() };

    while let Some(event) = tokio::select! {
        _ = cancellation.cancelled() =>
            return Err(AgentError::Cancelled),

        event = stream.next() =>
            event,
    } {
        match event? {
            ProviderEvent::TextDelta(delta) => {
                text.push_str(&delta);
                emit(TurnEvent::AssistantDelta { text: delta });
            }

            ProviderEvent::ThinkingDelta(delta) => {
                thinking.push_str(&delta);
                emit(TurnEvent::ThinkingDelta { text: delta });
            }

            ProviderEvent::ToolCall(call) => {
                tool_calls.push(call);
            }

            ProviderEvent::MessageStop { stop_reason: s, usage: u, model: m } => {
                stop_reason = s;
                usage = u;
                model = m;
            }

            _ => {}
        }
    }
    message
        .blocks
        .push(crate::model::message::ContentBlock::Text(crate::model::message::TextBlock { text }));
    message.blocks.push(crate::model::message::ContentBlock::Thinking(
        crate::model::message::ThinkingBlock {
            text: thinking,
            signature: None,
            is_redacted: false,
        },
    ));
    message
        .blocks
        .extend(tool_calls.into_iter().map(crate::model::message::ContentBlock::ToolCall));
    Ok(CompletionResponse { message, stop_reason, usage, model })
}

pub(super) async fn call_with_retry<P>(
    session: &mut SessionInfo,
    context: &mut Conversation,
    state: &mut RuntimeState,
    provider: &P,
    system_prompt_blocks: &[PromptBlock],
    tool_definitions: &[ToolDefinition],
    hint: ModelHint,
) -> Result<(Message, StopReason, Option<TokenUsage>, Option<String>, Vec<TurnEvent>), AgentError>
where
    P: ModelProvider,
{
    let max_tokens = provider.max_tokens();
    let mut retries = 0usize;
    let mut events = Vec::new();

    loop {
        if session.config().cancellation.is_cancelled() {
            return Err(AgentError::Cancelled);
        }

        let request = CompletionRequest {
            system_prompt_blocks: system_prompt_blocks.to_vec(),
            messages: context.messages().to_vec(),
            tools: tool_definitions.to_vec(),
            model_hint: Some(hint),
            max_tokens: Some(max_tokens),
        };
        let cancellation = session.config().cancellation.clone();
        match call_provider(request, provider, &cancellation, |event| {
            session.emit_turn_event(&event)
        })
        .await
        {
            Ok(response) => {
                return Ok((
                    response.message,
                    response.stop_reason,
                    response.usage,
                    response.model,
                    events,
                ));
            }
            Err(e) => {
                if e.is_context_too_long()
                    && let Some(compaction) = session.config().compaction.clone()
                {
                    warn!(error = %e, "context window exceeded; attempting compaction");
                    match compaction.compact(context.messages_mut(), provider).await {
                        Ok(true) => {
                            info!("reactive compaction succeeded");
                            state.metrics_mut().add_compaction();
                            context.push_system_reminder(
                                crate::model::message::SystemReminder::Compaction {
                                    reason: "reactive".into(),
                                },
                            );
                            events.push(TurnEvent::CompactionStarted { reason: "reactive".into() });
                            events
                                .push(TurnEvent::CompactionCompleted { reason: "reactive".into() });
                            for event in events.iter().rev().take(2).rev() {
                                session.emit_turn_event(event);
                            }
                            continue;
                        }
                        Ok(false) => warn!("reactive compaction had no effect"),
                        Err(compaction_error) => {
                            warn!(error = %compaction_error, "reactive compaction failed")
                        }
                    }
                }

                if e.is_retryable() && retries < session.config().retry.max_retries {
                    retries += 1;
                    let delay = session.config().retry.delay_for(retries);
                    state.metrics_mut().add_retry();
                    events.push(TurnEvent::ProviderRetry {
                        attempt: retries,
                        max_retries: session.config().retry.max_retries,
                        delay_ms: delay.as_millis() as u64,
                    });
                    if let Some(event) = events.last() {
                        session.emit_turn_event(event);
                    }
                    warn!(attempt = retries, error = %e, "provider call failed; retrying");
                    tokio::select! {
                        _ = cancellation.cancelled() => return Err(AgentError::Cancelled),
                        _ = tokio::time::sleep(delay) => {}
                    }
                    continue;
                }

                if e.is_retryable() {
                    error!(attempts = retries + 1, error = %e, "provider retries exhausted");
                    return Err(AgentError::ProviderRetriesExhausted {
                        attempts: retries + 1,
                        last_error: e.to_string(),
                    });
                }
                return Err(e);
            }
        }
    }
}
