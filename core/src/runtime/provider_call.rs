use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::config::{AgentConfig, TaskPath};
use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role, TextBlock, ThinkingBlock};
use crate::provider::{
    CompletionRequest, ModelHint, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};
use crate::runtime::{AgentSession, TurnEvent};
use crate::tool::ToolDefinition;

impl AgentSession {
    /// Determine the appropriate [`ModelHint`] for the current iteration.
    pub(crate) fn resolve_hint(
        config: &AgentConfig,
        iteration: usize,
        previous_tool_error: bool,
        consecutive_noop: usize,
    ) -> ModelHint {
        let (hint, reason) = if config.path == TaskPath::Fast {
            (ModelHint::Execution, "fast path")
        } else if previous_tool_error {
            (ModelHint::Recovery, "tool error")
        } else if consecutive_noop >= 3 {
            (ModelHint::Thinking, "stuck detection")
        } else if iteration == 1 {
            (ModelHint::Thinking, "first iteration")
        } else if config.path == TaskPath::Heavy && iteration.is_multiple_of(4) {
            (ModelHint::Thinking, "heavy periodic rethink")
        } else {
            (ModelHint::Execution, "default")
        };

        tracing::debug!(
            iteration = iteration,
            hint = ?hint,
            reason = reason,
            path = ?config.path,
            previous_tool_error = previous_tool_error,
            "hint resolved"
        );

        hint
    }

    /// Stream a single provider completion, handling retries.
    ///
    /// Returns the assistant message, stop reason, optional token usage, and all
    /// events that should be yielded during the call (deltas, thinking deltas,
    /// retry notifications).
    pub(super) async fn call_provider<P: ModelProvider>(
        &mut self,
        provider: &P,
        tool_definitions: &[ToolDefinition],
        hint: ModelHint,
    ) -> Result<(Message, StopReason, Option<TokenUsage>, Option<String>, Vec<TurnEvent>), AgentError>
    {
        let mut events = Vec::new();
        let mut attempts = 0;

        loop {
            attempts += 1;
            if self.config.cancellation.is_cancelled() {
                return Err(AgentError::Cancelled);
            }

            // Render the system prompt once and cache it. Re-rendering on
            // every provider call breaks DeepSeek's prefix caching: even a
            // single byte change in the system message invalidates the entire
            // cache for that request.
            if self.cached_system_prompt.is_none()
                && let Some(assembly) = &self.config.prompt_assembly
            {
                let blocks = assembly.build_blocks().await;
                let section_stats = blocks
                    .iter()
                    .map(|block| format!("{}:{} chars", block.name, block.text.chars().count()))
                    .collect::<Vec<_>>();
                let total_chars: usize = blocks.iter().map(|block| block.text.chars().count()).sum();
                info!(
                    prompt_profile = ?self.config.prompt_profile,
                    prompt_sections = blocks.len(),
                    prompt_total_chars = total_chars,
                    prompt_section_stats = ?section_stats,
                    "built system prompt"
                );
                self.cached_system_prompt =
                    Some(blocks.into_iter().map(|b| b.text).collect::<Vec<_>>().join("\n\n"));
            }
            let (system_prompt, system_prompt_blocks) = if self.cached_system_prompt.is_some() {
                (self.cached_system_prompt.clone(), None)
            } else {
                (self.config.base_system_prompt.clone(), None)
            };

            let request = CompletionRequest {
                system_prompt,
                system_prompt_blocks,
                messages: self.messages.clone(),
                tools: tool_definitions.to_vec(),
                model_hint: Some(hint),
                max_tokens: Some(128_000),
            };

            let mut stream = Box::pin(provider.stream_complete(request));
            let mut blocks = Vec::new();
            let mut stop_reason = StopReason::EndTurn;
            let mut usage = None;
            let mut model = None;
            let mut text_buf: Option<String> = None;
            let mut thinking_buf: Option<String> = None;
            let mut stream_error: Option<AgentError> = None;
            let cancellation = self.config.cancellation.clone();

            while let Some(event) = tokio::select! {
                _ = cancellation.cancelled() => return Err(AgentError::Cancelled),
                event = stream.next() => event,
            } {
                if self.config.cancellation.is_cancelled() {
                    return Err(AgentError::Cancelled);
                }
                match event {
                    Ok(ProviderEvent::MessageStart) => {}
                    Ok(ProviderEvent::TextDelta(text)) => {
                        events.push(TurnEvent::AssistantDelta { text: text.clone() });
                        text_buf.get_or_insert_with(String::new).push_str(&text);
                    }
                    Ok(ProviderEvent::ThinkingDelta(text)) => {
                        events.push(TurnEvent::ThinkingDelta { text: text.clone() });
                        thinking_buf.get_or_insert_with(String::new).push_str(&text);
                    }
                    Ok(ProviderEvent::ToolCall(call)) => {
                        if let Some(t) = text_buf.take() {
                            blocks.push(ContentBlock::Text(TextBlock { text: t }));
                        }
                        if let Some(t) = thinking_buf.take() {
                            blocks.push(ContentBlock::Thinking(ThinkingBlock {
                                text: t,
                                signature: None,
                                is_redacted: false,
                            }));
                        }
                        blocks.push(ContentBlock::ToolCall(call));
                    }
                    Ok(ProviderEvent::MessageStop {
                        stop_reason: reason,
                        usage: event_usage,
                        model: event_model,
                    }) => {
                        stop_reason = reason;
                        usage = event_usage;
                        model = event_model;
                    }
                    Err(e) => {
                        stream_error = Some(e);
                        break;
                    }
                }
            }

            if let Some(e) = stream_error {
                if self.config.retry.should_retry(&e, attempts) {
                    let delay = self.config.retry.delay_for(attempts);
                    warn!(
                        attempt = attempts,
                        delay_ms = delay.as_millis() as u64,
                        error = %e,
                        "provider call failed, retrying"
                    );
                    self.metrics.add_retry();
                    events.push(TurnEvent::ProviderRetry {
                        attempt: attempts,
                        max_retries: self.config.retry.max_retries,
                        delay_ms: delay.as_millis() as u64,
                    });
                    let cancellation = self.config.cancellation.clone();
                    tokio::select! {
                        _ = cancellation.cancelled() => return Err(AgentError::Cancelled),
                        _ = tokio::time::sleep(delay) => {}
                    }
                    continue;
                }

                // Reactive compaction: if the context window was exceeded,
                // compact the conversation and retry once rather than failing.
                if e.is_context_too_long()
                    && let Some(compaction) = self.config.compaction.clone()
                {
                    warn!(
                        error = %e,
                        "context window exceeded — attempting reactive compaction"
                    );
                    match compaction.compact(&mut self.messages, provider).await {
                        Ok(true) => {
                            info!("reactive compaction succeeded, retrying provider call");
                            self.metrics.add_compaction();
                            self.push_system_reminder(crate::message::SystemReminder::Compaction {
                                reason: "reactive".into(),
                            });
                            // Yield a compaction event so consumers know.
                            events.push(TurnEvent::CompactionStarted { reason: "reactive".into() });
                            events
                                .push(TurnEvent::CompactionCompleted { reason: "reactive".into() });
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
                    return Err(AgentError::ProviderRetriesExhausted {
                        attempts,
                        last_error: e.to_string(),
                    });
                } else {
                    return Err(e);
                }
            }

            if let Some(t) = text_buf.take() {
                blocks.push(ContentBlock::Text(TextBlock { text: t }));
            }
            if let Some(t) = thinking_buf.take() {
                blocks.push(ContentBlock::Thinking(ThinkingBlock {
                    text: t,
                    signature: None,
                    is_redacted: false,
                }));
            }
            return Ok((
                Message { role: Role::Assistant, blocks },
                stop_reason,
                usage,
                model,
                events,
            ));
        }
    }
}
