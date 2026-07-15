use futures_core::Stream;
use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::config::CancellationState;
use crate::error::AgentError;
use crate::message::{Message, Role};
use crate::prompt::PromptBlock;
use crate::provider::{
    CompletionRequest, CompletionResponse, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};
use crate::{ContextOps, ModelHint, ProviderError, SessionOps, StateOps, ToolDefinition, TurnEvent};

pub async fn call_provider<P: ModelProvider>(
    request: CompletionRequest,
    provider: &P,
    cancellation: &CancellationState,
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
            }

            ProviderEvent::ThinkingDelta(delta) => {
                thinking.push_str(&delta);
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
    message.blocks.push(crate::message::ContentBlock::Text(crate::message::TextBlock { text }));
    message.blocks.push(crate::message::ContentBlock::Thinking(crate::message::ThinkingBlock {
        text: thinking,
        signature: None,
        is_redacted: false,
    }));
    message.blocks.extend(tool_calls.into_iter().map(crate::message::ContentBlock::ToolCall));
    Ok(CompletionResponse { message, stop_reason, usage, model })
}

/// Call the provider with retry and reactive compaction on context-window errors.
///
/// Returns the assistant message, stop reason, token usage, model identifier,
/// and a list of [`TurnEvent`]s emitted during the call (delta events, retry
/// events, compaction events).
pub(super) async fn call_with_retry<S, C, St, P>(
    session: &mut S,
    context: &mut C,
    state: &mut St,
    provider: &P,
    system_prompt_blocks: &[PromptBlock],
    tool_definitions: &[ToolDefinition],
    hint: ModelHint,
) -> Result<CompletionResponse, AgentError>
where
    S: SessionOps,
    C: ContextOps,
    St: StateOps,
    P: ModelProvider,
{
    let request = CompletionRequest {
        system_prompt_blocks: system_prompt_blocks.to_vec(),
        messages: context.messages().to_vec(),
        tools: tool_definitions.to_vec(),
        model_hint: Some(hint),
        max_tokens: Some(128_000),
    };
    let mut attempts = 0;
    let mut all_events = Vec::new();
    loop {
        attempts += 1;
        if session.config().cancellation.is_cancelled() {
            Err(AgentError::Cancelled)?;
        }

        let cancellation = session.config().cancellation.clone();
        match call_provider(request.clone(), provider, &cancellation).await {
            Ok(completion_response) => {
                return Ok(completion_response);
            }
            Err(e) => {}
        }
    }
}

// Convert an AgentError into a TurnEvent, handling retries and compaction if applicable.
async fn error_to_event(e: AgentError) -> TurnEvent {
    if session.config().retry.should_retry(&e, attempts) {
        let delay = session.config().retry.delay_for(attempts);
        warn!(
            attempt = attempts,
            delay_ms = delay.as_millis() as u64,
            error = %e,
            "provider call failed, retrying"
        );
        state.metrics_mut().add_retry();
        all_events.push(TurnEvent::ProviderRetry {
            attempt: attempts,
            max_retries: session.config().retry.max_retries,
            delay_ms: delay.as_millis() as u64,
        });
        {
            let cancellation = session.config().cancellation.clone();
            tokio::select! {
                _ = cancellation.cancelled() => {},
                _ = tokio::time::sleep(delay) => {}
            }
        }
        if session.config().cancellation.is_cancelled() {
            Err(AgentError::Cancelled)?
        }
        continue;
    }

    if e.is_context_too_long()
        && let Some(compaction) = session.config().compaction.clone()
    {
        warn!(
            error = %e,
            "context window exceeded — attempting reactive compaction"
        );
        match compaction.compact(context.messages_mut(), provider).await {
            Ok(true) => {
                info!("reactive compaction succeeded, retrying provider call");
                state.metrics_mut().add_compaction();
                context.push_system_reminder(crate::message::SystemReminder::Compaction {
                    reason: "reactive".into(),
                });
                all_events.push(TurnEvent::CompactionStarted { reason: "reactive".into() });
                all_events.push(TurnEvent::CompactionCompleted { reason: "reactive".into() });
                continue;
            }
            Ok(false) => {
                warn!("reactive compaction had no effect — context still too large");
            }
            Err(compact_err) => {
                warn!(
                    error = %compact_err,
                    "reactive compaction failed"
                );
            }
        }
    }

    if e.is_retryable() {
        error!(attempts, error = %e, "provider retries exhausted");
        Err(AgentError::ProviderRetriesExhausted { attempts, last_error: e.to_string() })?
    } else {
        Err(e)?
    }
}
