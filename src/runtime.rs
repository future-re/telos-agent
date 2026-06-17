//! Agent session and turn loop — the orchestration core of the crate.
//!
//! An [`AgentSession`] owns the conversation history and exposes two ways to
//! run a turn:
//! - [`AgentSession::run_turn_stream`] — yields [`TurnEvent`]s incrementally
//!   for live UIs.
//! - [`AgentSession::run_turn`] — collects the stream into a [`TurnResult`]
//!   and persists the session afterwards.
//!
//! A turn is `(model → optional tool calls → model → …)` until the model
//! stops or `max_iterations` is hit.

use async_stream::try_stream;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, info, info_span, warn};

use crate::compaction::{CompactionConfig, compact_tool_result_message};
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::executor::{ToolExecutionEvent, ToolExecutionStreamItem, execute_tool_calls_stream};
use crate::hooks::{HookContext, HookPhase};
use crate::message::{ContentBlock, Message, Role, TextBlock};
use crate::metrics::SessionMetrics;
use crate::provider::{
    CompletionRequest, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};
use crate::storage::Storage;
use crate::tool::FileReadState;
use crate::tool::ToolRegistry;

/// Monotonic counter used to mint unique session identifiers within a process.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Streaming event emitted during a single turn of the agent loop.
///
/// Events are emitted in causal order — e.g. an [`AssistantDelta`](Self::AssistantDelta)
/// for each streamed text fragment, then [`Assistant`](Self::Assistant) once
/// the full message is materialised, then per-tool events if the model
/// requested tool calls.
#[derive(Debug, Clone, Serialize)]
pub enum TurnEvent {
    /// Fired exactly once at the start of a turn with the user's input.
    TurnStarted {
        session_id: String,
        turn_id: u64,
        user_input: String,
    },
    /// Fired at the top of each model ⇄ tool iteration within the turn.
    IterationStarted {
        iteration: usize,
        message_count: usize,
    },
    /// About to issue a completion request to the provider.
    ProviderRequest {
        iteration: usize,
        message_count: usize,
        tool_count: usize,
    },
    /// Provider reported token usage for the just-finished iteration.
    ProviderUsage {
        input_tokens: usize,
        output_tokens: usize,
    },
    /// Incremental text fragment streamed from the assistant.
    AssistantDelta { text: String },
    /// The full user message that was just appended to the conversation.
    User(Message),
    /// A completed assistant message (either model output or hook-emitted).
    Assistant(Message),
    /// A tool call has begun executing.
    ToolCall { tool_call_id: String, name: String },
    /// Progress update emitted from inside a long-running tool.
    ToolProgress {
        tool_call_id: Option<String>,
        name: String,
        message: String,
        data: Option<serde_json::Value>,
    },
    /// A tool call finished (successfully or with an error).
    ToolCompleted {
        tool_call_id: String,
        name: String,
        is_error: bool,
    },
    /// The aggregated tool-result message appended to the conversation.
    ToolResult(Message),
    /// A compaction pass is starting; `reason` identifies which threshold tripped.
    CompactionStarted { reason: String },
    /// A compaction pass finished.
    CompactionCompleted { reason: String },
    /// Estimated request size exceeded [`TokenBudget::max_tokens`](crate::TokenBudget::max_tokens);
    /// the turn ends without calling the model.
    TokenBudgetExceeded {
        used_tokens: usize,
        max_tokens: usize,
    },
    /// A registered hook is starting.
    HookStarted { phase: &'static str, name: String },
    /// A registered hook finished; `emitted_message` is `true` if it appended a follow-up.
    HookCompleted {
        phase: &'static str,
        name: String,
        emitted_message: bool,
    },
    /// A provider call failed with a retryable error and is being retried.
    ProviderRetry {
        attempt: usize,
        max_retries: usize,
        delay_ms: u64,
    },
    /// Final event of a turn — the assistant produced an end-of-turn message.
    TurnFinished {
        stop_reason: StopReason,
        final_text: String,
    },
}

/// Collected result of a turn, returned by [`AgentSession::run_turn`].
#[derive(Debug, Clone, Serialize)]
pub struct TurnResult {
    /// Every event emitted during the turn, in order.
    pub events: Vec<TurnEvent>,
    /// The last assistant message seen (the answer the caller usually wants).
    pub final_message: Message,
    /// Why the turn stopped — informational for callers.
    pub stop_reason: StopReason,
}

/// An agent session that maintains conversation state across turns.
///
/// Created via [`AgentSession::new`] or [`AgentSession::resume`].
pub struct AgentSession {
    config: AgentConfig,
    session_id: String,
    /// Monotonic counter; incremented at the start of each turn.
    next_turn_id: u64,
    /// Full conversation, including the optional leading system prompt.
    messages: Vec<Message>,
    /// Shared state used by filesystem tools to reject stale writes.
    read_file_state: FileReadState,
    /// Accumulated session-level metrics updated by the runtime.
    metrics: SessionMetrics,
}

impl AgentSession {
    /// Start a fresh session. If `config.system_prompt` is set, it is appended
    /// as the first message.
    pub fn new(config: AgentConfig) -> Self {
        let mut messages = Vec::new();
        if let Some(system_prompt) = config.system_prompt.as_ref() {
            messages.push(Message::system(system_prompt.clone()));
        }

        Self {
            config,
            session_id: format!(
                "session-{}",
                NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            ),
            next_turn_id: 1,
            messages,
            read_file_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            metrics: SessionMetrics::new(),
        }
    }

    /// Snapshot of the current conversation.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Unique identifier minted at construction (or supplied to [`resume`](Self::resume)).
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Snapshot of the accumulated session metrics.
    pub fn metrics(&self) -> &SessionMetrics {
        &self.metrics
    }

    /// Drop all non-system messages and reset the turn counter.
    pub fn reset(&mut self) {
        self.messages
            .retain(|message| message.role == crate::message::Role::System);
        self.next_turn_id = 1;
        self.read_file_state = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    }

    /// Persist the conversation if a [`Storage`] backend is configured.
    pub async fn save(&self) -> Result<(), AgentError> {
        if let Some(storage) = &self.config.storage {
            storage
                .save_snapshot(&self.session_id, &self.messages)
                .await?;
        }
        Ok(())
    }

    /// Resume a previously persisted session from `storage`.
    ///
    /// If the loaded transcript has a different system prompt than `config`,
    /// the config's prompt wins — the loaded one is overwritten so the session
    /// behaves consistently across restarts.
    pub async fn resume(
        session_id: impl Into<String>,
        mut config: AgentConfig,
        storage: Arc<dyn Storage>,
    ) -> Result<Self, AgentError> {
        let session_id = session_id.into();
        let mut messages = storage.load(&session_id).await?;
        if messages.is_empty() {
            if let Some(system_prompt) = config.system_prompt.as_ref() {
                messages.push(Message::system(system_prompt.clone()));
            }
        } else {
            // Ensure the loaded system prompt matches config
            let loaded_system = messages
                .first()
                .filter(|m| m.role == crate::message::Role::System)
                .map(|m| m.text_content());
            if let Some(config_system) = &config.system_prompt {
                if loaded_system.as_deref() != Some(config_system.as_str()) {
                    // Replace system prompt if config differs
                    if messages.first().map(|m| m.role) == Some(crate::message::Role::System) {
                        messages[0] = Message::system(config_system.clone());
                    } else {
                        messages.insert(0, Message::system(config_system.clone()));
                    }
                }
            }
        }
        config.storage = Some(storage);
        Ok(Self {
            config,
            session_id,
            next_turn_id: 1,
            messages,
            read_file_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            metrics: SessionMetrics::new(),
        })
    }

    /// Run one turn, yielding [`TurnEvent`]s as the turn progresses.
    ///
    /// The stream borrows `self` mutably so the conversation is updated in
    /// place as events are produced. Errors abort the stream; partially
    /// produced events up to that point are still observed by the consumer.
    pub fn run_turn_stream<'a, P: ModelProvider + 'a>(
        &'a mut self,
        provider: &'a P,
        tools: &'a ToolRegistry,
        user_input: impl Into<String> + 'a,
    ) -> impl Stream<Item = Result<TurnEvent, AgentError>> + 'a {
        try_stream! {
            let turn_id = self.next_turn_id;
            self.next_turn_id += 1;
            let user_input = user_input.into();
            let user_message = Message::user(user_input.clone());
            self.messages.push(user_message.clone());

            yield TurnEvent::TurnStarted {
                session_id: self.session_id.clone(),
                turn_id,
                user_input,
            };
            yield TurnEvent::User(user_message);

            self.metrics.add_turn();

            {
                let _guard = info_span!("turn", session_id = %self.session_id, turn_id).entered();
                info!("turn started");
            }

            let mut iterations = 0;
            loop {
                // Bail out if the model keeps calling tools forever. The cap
                // also protects against pathological tool-result loops.
                if iterations >= self.config.max_iterations {
                    Err(AgentError::MaxIterations(self.config.max_iterations))?;
                }
                iterations += 1;
                self.metrics.add_iteration();

                // Check for cancellation before each iteration.
                if self.config.cancelled.load(Ordering::Relaxed) {
                    warn!("turn cancelled during iteration {}", iterations);
                    Err(AgentError::Cancelled)?;
                }

                let tool_definitions = tools.definitions();
                {
                    let _guard = info_span!("iteration", iteration = iterations, messages = self.messages.len()).entered();
                    debug!("iteration started");
                }

                yield TurnEvent::IterationStarted {
                    iteration: iterations,
                    message_count: self.messages.len(),
                };
                yield TurnEvent::ProviderRequest {
                    iteration: iterations,
                    message_count: self.messages.len(),
                    tool_count: tool_definitions.len(),
                };

                // Two compaction passes:
                // 1. Token-budget compaction — fires early (at compact_at_tokens) so the
                //    model never sees a request that exceeds the hard limit.
                // 2. General compaction — an optional second strategy (e.g. char-based).
                if let Some(budget) = self.config.token_budget {
                    let estimated_tokens = estimate_message_tokens(&self.messages, provider);
                    if estimated_tokens > budget.max_tokens {
                        warn!(
                            used_tokens = estimated_tokens,
                            max_tokens = budget.max_tokens,
                            "token budget exceeded"
                        );
                        yield TurnEvent::TokenBudgetExceeded {
                            used_tokens: estimated_tokens,
                            max_tokens: budget.max_tokens,
                        };
                        yield TurnEvent::TurnFinished {
                            stop_reason: StopReason::EndTurn,
                            final_text: String::new(),
                        };
                        break;
                    }
                    if estimated_tokens >= budget.compact_at_tokens {
                        if let Some(compaction) = self.config.compaction.clone() {
                            if compaction.compact(&mut self.messages, provider).await? {
                                self.metrics.add_compaction();
                                info!("token-budget compaction applied");
                                yield TurnEvent::CompactionStarted {
                                    reason: "token_budget".into(),
                                };
                                yield TurnEvent::CompactionCompleted {
                                    reason: "token_budget".into(),
                                };
                            }
                        }
                    }
                }

                if let Some(compaction) = self.config.compaction.clone() {
                    if compaction.compact(&mut self.messages, provider).await? {
                        self.metrics.add_compaction();
                        info!("char-budget compaction applied");
                        yield TurnEvent::CompactionStarted {
                            reason: "char_budget".into(),
                        };
                        yield TurnEvent::CompactionCompleted {
                            reason: "char_budget".into(),
                        };
                    }
                }

                let (assistant_message, stop_reason, usage) = {
                    let mut attempts = 0;
                    loop {
                        attempts += 1;
                        // Honour cancellation between retries (and before the first attempt).
                        if self.config.cancelled.load(Ordering::Relaxed) {
                            Err(AgentError::Cancelled)?;
                        }
                        let request = CompletionRequest {
                            system_prompt: self.config.system_prompt.clone(),
                            messages: self.messages.clone(),
                            tools: tool_definitions.clone(),
                        };
                        // Drive the provider stream to completion, accumulating
                        // text and tool-call blocks into a single assistant
                        // message. Consecutive TextDelta events are merged into a
                        // single TextBlock so text_content() doesn't inject
                        // spurious newlines.
                        let mut stream = Box::pin(provider.stream_complete(request));
                        let mut blocks = Vec::new();
                        let mut stop_reason = StopReason::EndTurn;
                        let mut usage = None;
                        let mut text_buf: Option<String> = None;
                        let mut stream_error: Option<AgentError> = None;
                        while let Some(event) = stream.next().await {
                            match event {
                                Ok(ProviderEvent::MessageStart) => {}
                                Ok(ProviderEvent::TextDelta(text)) => {
                                    yield TurnEvent::AssistantDelta {
                                        text: text.clone(),
                                    };
                                    text_buf
                                        .get_or_insert_with(String::new)
                                        .push_str(&text);
                                }
                                Ok(ProviderEvent::ToolCall(call)) => {
                                    // Flush buffered text before the tool call.
                                    if let Some(t) = text_buf.take() {
                                        blocks.push(ContentBlock::Text(TextBlock {
                                            text: t,
                                        }));
                                    }
                                    blocks.push(ContentBlock::ToolCall(call));
                                }
                                Ok(ProviderEvent::MessageStop {
                                    stop_reason: reason,
                                    usage: event_usage,
                                }) => {
                                    stop_reason = reason;
                                    usage = event_usage;
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
                                yield TurnEvent::ProviderRetry {
                                    attempt: attempts,
                                    max_retries: self.config.retry.max_retries,
                                    delay_ms: delay.as_millis() as u64,
                                };
                                tokio::time::sleep(delay).await;
                                continue;
                            }
                            error!(
                                attempts,
                                error = %e,
                                "provider retries exhausted"
                            );
                            Err(AgentError::ProviderRetriesExhausted {
                                attempts,
                                last_error: e.to_string(),
                            })?;
                        }
                        // Flush any remaining buffered text after the stream ends.
                        if let Some(t) = text_buf.take() {
                            blocks.push(ContentBlock::Text(TextBlock { text: t }));
                        }
                        break (
                            Message {
                                role: Role::Assistant,
                                blocks,
                            },
                            stop_reason,
                            usage,
                        );
                    }
                };
                if let Some(TokenUsage { input_tokens, output_tokens }) = usage {
                    self.metrics.add_input_tokens(input_tokens);
                    self.metrics.add_output_tokens(output_tokens);
                    debug!(
                        input_tokens,
                        output_tokens,
                        "provider usage"
                    );
                    yield TurnEvent::ProviderUsage {
                        input_tokens,
                        output_tokens,
                    };
                }
                self.messages.push(assistant_message.clone());
                yield TurnEvent::Assistant(assistant_message.clone());

                let hook_context = HookContext {
                    session_id: self.session_id.clone(),
                    turn_id,
                    message_count: self.messages.len(),
                };

                // Run PostSampling hooks every iteration — including iterations
                // that end with a tool call. Each hook may emit an extra
                // message that gets appended to the conversation.
                for hook in self.config.hooks.hooks_for_phase(HookPhase::PostSampling) {
                    yield TurnEvent::HookStarted {
                        phase: "post_sampling",
                        name: hook.name().to_string(),
                    };
                    let maybe_message = hook.run(&hook_context, &assistant_message).await?;
                    let emitted = maybe_message.is_some();
                    if let Some(message) = maybe_message {
                        self.messages.push(message.clone());
                        yield TurnEvent::Assistant(message);
                    }
                    yield TurnEvent::HookCompleted {
                        phase: "post_sampling",
                        name: hook.name().to_string(),
                        emitted_message: emitted,
                    };
                }

                let tool_calls = assistant_message.tool_calls().cloned().collect::<Vec<_>>();
                if tool_calls.is_empty() {
                    // No tool calls pending — run Stop hooks and end the turn.
                    for hook in self.config.hooks.hooks_for_phase(HookPhase::Stop) {
                        yield TurnEvent::HookStarted {
                            phase: "stop",
                            name: hook.name().to_string(),
                        };
                        let maybe_message = hook.run(&hook_context, &assistant_message).await?;
                        let emitted = maybe_message.is_some();
                        if let Some(message) = maybe_message {
                            self.messages.push(message.clone());
                            yield TurnEvent::Assistant(message);
                        }
                        yield TurnEvent::HookCompleted {
                            phase: "stop",
                            name: hook.name().to_string(),
                            emitted_message: emitted,
                        };
                    }

                    yield TurnEvent::TurnFinished {
                        stop_reason,
                        final_text: assistant_message.text_content(),
                    };
                    info!(?stop_reason, "turn finished");
                    break;
                }

                // Check for cancellation before executing tools.
                if self.config.cancelled.load(Ordering::Relaxed) {
                    Err(AgentError::Cancelled)?;
                }

                // Execute the requested tool calls. The executor batches
                // concurrency-safe tools and interleaves progress events with
                // result events.
                let mut execution = Box::pin(execute_tool_calls_stream(
                    tool_calls,
                    tools,
                    &self.config,
                    &self.session_id,
                    turn_id,
                    self.messages.clone(),
                    self.read_file_state.clone(),
                ));
                let mut tool_results = Vec::new();
                while let Some(item) = execution.next().await {
                    match item {
                        ToolExecutionStreamItem::Event(event) => {
                            match event {
                                ToolExecutionEvent::ToolStarted { tool_call_id, name } => {
                                    yield TurnEvent::ToolCall { tool_call_id, name };
                                }
                                ToolExecutionEvent::ToolProgress {
                                    tool_call_id,
                                    name,
                                    message,
                                    data,
                                } => {
                                    yield TurnEvent::ToolProgress {
                                        tool_call_id,
                                        name,
                                        message,
                                        data,
                                    };
                                }
                                ToolExecutionEvent::ToolCompleted {
                                    tool_call_id,
                                    name,
                                    is_error,
                                } => {
                                    yield TurnEvent::ToolCompleted {
                                        tool_call_id,
                                        name,
                                        is_error,
                                    };
                                }
                            }
                        }
                        ToolExecutionStreamItem::Result(result) => {
                            tool_results.push(result);
                        }
                    }
                }

                // Count tool calls and errors for metrics.
                for result in &tool_results {
                    self.metrics.add_tool_call();
                    if result.is_error {
                        self.metrics.add_tool_error();
                    }
                }

                // Bundle every tool result into a single tool-role message so
                // the next iteration sees them all at once. Truncate any
                // oversized payloads first so the model isn't drowned.
                let tool_message = Message::tool_results(tool_results);
                let compaction_config = CompactionConfig {
                    max_tool_result_chars: self.config.max_tool_result_chars,
                };
                let compaction = compact_tool_result_message(tool_message, &compaction_config);
                if compaction.compacted {
                    yield TurnEvent::CompactionStarted {
                        reason: "tool_result_budget".into(),
                    };
                    yield TurnEvent::CompactionCompleted {
                        reason: "tool_result_budget".into(),
                    };
                }
                self.messages.push(compaction.message.clone());
                yield TurnEvent::ToolResult(compaction.message);
            }
        }
    }

    /// Run one turn to completion and return the collected events plus the
    /// final message. Persists the session to [`Storage`] (if configured)
    /// before returning.
    pub async fn run_turn<P: ModelProvider>(
        &mut self,
        provider: &P,
        tools: &ToolRegistry,
        user_input: impl Into<String>,
    ) -> Result<TurnResult, AgentError> {
        let (events, final_message, stop_reason) = {
            let mut stream = Box::pin(self.run_turn_stream(provider, tools, user_input));
            let mut events = Vec::new();
            let mut final_message = None;
            let mut stop_reason = StopReason::EndTurn;

            while let Some(event) = stream.next().await {
                let event = event?;
                if let TurnEvent::Assistant(message) = &event {
                    final_message = Some(message.clone());
                }
                if let TurnEvent::TurnFinished {
                    stop_reason: reason,
                    ..
                } = event.clone()
                {
                    stop_reason = reason;
                }
                events.push(event);
            }
            (events, final_message, stop_reason)
        };

        self.save().await?;

        Ok(TurnResult {
            final_message: final_message.unwrap_or_else(|| Message::assistant("")),
            events,
            stop_reason,
        })
    }
}

impl TurnEvent {
    /// Return the [`Message`] carried by this event, or panic.
    ///
    /// Only call on variants known to carry a message — i.e. [`User`](TurnEvent::User),
    /// [`Assistant`](TurnEvent::Assistant), [`ToolResult`](TurnEvent::ToolResult).
    pub fn message(&self) -> &Message {
        match self {
            TurnEvent::User(message)
            | TurnEvent::Assistant(message)
            | TurnEvent::ToolResult(message) => message,
            _ => panic!("event has no message"),
        }
    }

    /// Human-readable one-line summary of the event — useful for trace logs / CLIs.
    pub fn text(&self) -> String {
        match self {
            TurnEvent::TurnStarted {
                session_id,
                turn_id,
                user_input,
            } => format!("turn_started:{}#{}:{}", session_id, turn_id, user_input),
            TurnEvent::IterationStarted {
                iteration,
                message_count,
            } => format!("iteration_started:{} messages={}", iteration, message_count),
            TurnEvent::ProviderRequest {
                iteration,
                message_count,
                tool_count,
            } => format!(
                "provider_request:{} messages={} tools={}",
                iteration, message_count, tool_count
            ),
            TurnEvent::ProviderUsage {
                input_tokens,
                output_tokens,
            } => format!("provider_usage:input={input_tokens} output={output_tokens}"),
            TurnEvent::AssistantDelta { text } => format!("assistant_delta:{text}"),
            TurnEvent::ToolCall { tool_call_id, name } => {
                format!("tool_call:{}#{}", name, tool_call_id)
            }
            TurnEvent::ToolProgress {
                tool_call_id,
                name,
                message,
                ..
            } => format!(
                "tool_progress:{}#{}:{}",
                name,
                tool_call_id.as_deref().unwrap_or("unknown"),
                message
            ),
            TurnEvent::ToolCompleted {
                tool_call_id,
                name,
                is_error,
            } => format!(
                "tool_completed:{}#{} error={}",
                name, tool_call_id, is_error
            ),
            TurnEvent::CompactionStarted { reason } => {
                format!("compaction_started:{reason}")
            }
            TurnEvent::CompactionCompleted { reason } => {
                format!("compaction_completed:{reason}")
            }
            TurnEvent::TokenBudgetExceeded {
                used_tokens,
                max_tokens,
            } => format!("token_budget_exceeded:{used_tokens}/{max_tokens}"),
            TurnEvent::HookStarted { phase, name } => {
                format!("hook_started:{phase}:{name}")
            }
            TurnEvent::HookCompleted {
                phase,
                name,
                emitted_message,
            } => format!("hook_completed:{phase}:{name}:{emitted_message}"),
            TurnEvent::ProviderRetry {
                attempt,
                max_retries,
                delay_ms,
            } => {
                format!("provider_retry:{attempt}/{max_retries} delay={delay_ms}ms")
            }
            TurnEvent::TurnFinished {
                stop_reason,
                final_text,
            } => format!("turn_finished:{stop_reason:?}:{final_text}"),
            _ => self
                .message()
                .blocks
                .iter()
                .map(|block| match block {
                    ContentBlock::Text(text) => text.text.clone(),
                    ContentBlock::ToolCall(call) => {
                        format!("tool_call:{}({})", call.name, call.arguments)
                    }
                    ContentBlock::ToolResult(result) => {
                        format!("tool_result:{}={}", result.name, result.content)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// Sum estimated token counts across every block in `messages`.
///
/// Used by the turn loop to decide whether to invoke compaction or abort the
/// turn before issuing a request the model can't accept.
fn estimate_message_tokens(messages: &[Message], provider: &dyn ModelProvider) -> usize {
    messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .map(|block| match block {
            ContentBlock::Text(text) => provider.estimate_tokens(&text.text),
            ContentBlock::ToolCall(call) => {
                provider.estimate_tokens(&call.name)
                    + provider.estimate_tokens(&call.arguments.to_string())
            }
            ContentBlock::ToolResult(result) => {
                provider.estimate_tokens(&result.content.to_string())
            }
        })
        .sum()
}
