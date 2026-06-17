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
use crate::message::{ContentBlock, Message, Role, TextBlock, ThinkingBlock};
use crate::metrics::SessionMetrics;
use crate::provider::{
    CompletionRequest, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};
use crate::storage::{SessionMetadata, Storage};
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
    /// Incremental reasoning fragment streamed from a thinking-capable model.
    ThinkingDelta { text: String },
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
    /// A tool call has been suspended pending human approval.
    ApprovalRequested {
        tool_call_id: String,
        name: String,
        reason: String,
    },
    /// Human approval has been resolved for a suspended tool call.
    ApprovalResolved {
        tool_call_id: String,
        name: String,
        decision: String,
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

/// Outcome of the compaction phase at the top of an iteration.
enum CompactionResult {
    /// Compaction completed (or was skipped); caller should continue the turn.
    Continue {
        events: Vec<TurnEvent>,
        compactions: usize,
    },
    /// Token budget was already exceeded; caller should finish the turn early.
    AbortTurn {
        events: Vec<TurnEvent>,
    },
}

impl AgentSession {
    /// Start a fresh session. If `config.system_prompt` is set, it is appended
    /// as the first message.
    pub fn new(config: AgentConfig) -> Self {
        if let Err(err) = config.validate() {
            // Panic mirrors the fail-fast behaviour of other invalid configurations
            // (e.g. missing env vars) while keeping the constructor synchronous.
            panic!("invalid AgentConfig: {err}");
        }
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

    /// Persist the conversation and session metadata if a [`Storage`] backend is configured.
    pub async fn save(&self) -> Result<(), AgentError> {
        if let Some(storage) = &self.config.storage {
            storage
                .save_snapshot(&self.session_id, &self.messages)
                .await?;
            let read_file_state = self.read_file_state.lock().await.clone();
            let metadata = SessionMetadata {
                next_turn_id: self.next_turn_id,
                total_input_tokens: self.metrics.total_input_tokens(),
                total_output_tokens: self.metrics.total_output_tokens(),
                total_tool_calls: self.metrics.total_tool_calls(),
                total_tool_errors: self.metrics.total_tool_errors(),
                total_iterations: self.metrics.total_iterations(),
                compaction_count: self.metrics.compaction_count(),
                turn_count: self.metrics.turn_count(),
                retry_count: self.metrics.retry_count(),
                read_file_state,
            };
            storage.save_metadata(&self.session_id, &metadata).await?;
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
        config.validate()?;
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
            if let Some(config_system) = &config.system_prompt
                && loaded_system.as_deref() != Some(config_system.as_str())
            {
                // Replace system prompt if config differs
                if messages.first().map(|m| m.role) == Some(crate::message::Role::System) {
                    messages[0] = Message::system(config_system.clone());
                } else {
                    messages.insert(0, Message::system(config_system.clone()));
                }
            }
        }

        let metadata = storage.load_metadata(&session_id).await?;
        let (next_turn_id, metrics, read_file_state) = if let Some(m) = metadata {
            (
                m.next_turn_id,
                SessionMetrics::with_values(
                    m.total_input_tokens,
                    m.total_output_tokens,
                    m.total_tool_calls,
                    m.total_tool_errors,
                    m.total_iterations,
                    m.compaction_count,
                    m.turn_count,
                    m.retry_count,
                ),
                m.read_file_state,
            )
        } else {
            (
                1,
                SessionMetrics::new(),
                HashMap::new(),
            )
        };

        config.storage = Some(storage);
        Ok(Self {
            config,
            session_id,
            next_turn_id,
            messages,
            read_file_state: Arc::new(tokio::sync::Mutex::new(read_file_state)),
            metrics,
        })
    }

    /// Run token-budget and general compaction passes for the current iteration.
    ///
    /// Returns the events that should be yielded and the number of compactions
    /// that actually modified the conversation.
    async fn run_compaction_phase<P: ModelProvider>(
        &mut self,
        provider: &P,
        iteration: usize,
    ) -> Result<CompactionResult, AgentError> {
        let mut events = Vec::new();
        let mut compactions = 0;

        if let Some(budget) = self.config.token_budget {
            let estimated_tokens = estimate_message_tokens(&self.messages, provider);
            if estimated_tokens > budget.max_tokens {
                warn!(
                    used_tokens = estimated_tokens,
                    max_tokens = budget.max_tokens,
                    "token budget exceeded"
                );
                events.push(TurnEvent::TokenBudgetExceeded {
                    used_tokens: estimated_tokens,
                    max_tokens: budget.max_tokens,
                });
                return Ok(CompactionResult::AbortTurn { events });
            }
            if estimated_tokens >= budget.compact_at_tokens
                && let Some(compaction) = self.config.compaction.clone()
            {
                events.push(TurnEvent::CompactionStarted {
                    reason: "token_budget".into(),
                });
                let did_compact = compaction.compact(&mut self.messages, provider).await?;
                events.push(TurnEvent::CompactionCompleted {
                    reason: "token_budget".into(),
                });
                if did_compact {
                    compactions += 1;
                    info!(iteration, "token-budget compaction applied");
                }
            }
        }

        if let Some(compaction) = self.config.compaction.clone() {
            events.push(TurnEvent::CompactionStarted {
                reason: "char_budget".into(),
            });
            let did_compact = compaction.compact(&mut self.messages, provider).await?;
            events.push(TurnEvent::CompactionCompleted {
                reason: "char_budget".into(),
            });
            if did_compact {
                compactions += 1;
                info!(iteration, "char-budget compaction applied");
            }
        }

        Ok(CompactionResult::Continue { events, compactions })
    }

    /// Stream a single provider completion, handling retries.
    ///
    /// Returns the assistant message, stop reason, optional token usage, and all
    /// events that should be yielded during the call (deltas, thinking deltas,
    /// retry notifications).
    async fn call_provider<P: ModelProvider>(
        &mut self,
        provider: &P,
        tool_definitions: &[crate::tool::ToolDefinition],
    ) -> Result<(Message, StopReason, Option<TokenUsage>, Vec<TurnEvent>), AgentError> {
        let mut events = Vec::new();
        let mut attempts = 0;

        loop {
            attempts += 1;
            if self.config.cancelled.load(Ordering::Relaxed) {
                return Err(AgentError::Cancelled);
            }

            let request = CompletionRequest {
                system_prompt: self.config.system_prompt.clone(),
                messages: self.messages.clone(),
                tools: tool_definitions.to_vec(),
            };

            let mut stream = Box::pin(provider.stream_complete(request));
            let mut blocks = Vec::new();
            let mut stop_reason = StopReason::EndTurn;
            let mut usage = None;
            let mut text_buf: Option<String> = None;
            let mut thinking_buf: Option<String> = None;
            let mut stream_error: Option<AgentError> = None;

            while let Some(event) = stream.next().await {
                if self.config.cancelled.load(Ordering::Relaxed) {
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
                    events.push(TurnEvent::ProviderRetry {
                        attempt: attempts,
                        max_retries: self.config.retry.max_retries,
                        delay_ms: delay.as_millis() as u64,
                    });
                    tokio::time::sleep(delay).await;
                    continue;
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
                Message {
                    role: Role::Assistant,
                    blocks,
                },
                stop_reason,
                usage,
                events,
            ));
        }
    }

    /// Run all hooks registered for a given phase and append any emitted messages.
    async fn run_hook_phase(
        &mut self,
        phase: HookPhase,
        hook_context: &HookContext,
        assistant_message: &Message,
    ) -> Result<Vec<TurnEvent>, AgentError> {
        let mut events = Vec::new();
        let phase_name: &'static str = match phase {
            HookPhase::PostSampling => "post_sampling",
            HookPhase::Stop => "stop",
        };
        for hook in self.config.hooks.hooks_for_phase(phase) {
            events.push(TurnEvent::HookStarted {
                phase: phase_name,
                name: hook.name().to_string(),
            });
            let maybe_message = hook.run(hook_context, assistant_message).await?;
            let emitted = maybe_message.is_some();
            if let Some(message) = maybe_message {
                self.messages.push(message.clone());
                events.push(TurnEvent::Assistant(message));
            }
            events.push(TurnEvent::HookCompleted {
                phase: phase_name,
                name: hook.name().to_string(),
                emitted_message: emitted,
            });
        }
        Ok(events)
    }

    /// Execute a batch of tool calls and build the compacted tool-result message.
    async fn execute_tool_calls_phase(
        &mut self,
        tools: &ToolRegistry,
        tool_calls: Vec<crate::message::ToolCall>,
        turn_id: u64,
    ) -> Result<(Message, Vec<TurnEvent>), AgentError> {
        let mut events = Vec::new();
        let messages = Arc::new(self.messages.clone());
        let mut execution = Box::pin(execute_tool_calls_stream(
            tool_calls,
            tools,
            &self.config,
            &self.session_id,
            turn_id,
            messages,
            self.read_file_state.clone(),
        ));

        let mut tool_results = Vec::new();
        while let Some(item) = execution.next().await {
            match item {
                ToolExecutionStreamItem::Event(event) => {
                    let turn_event = match event {
                        ToolExecutionEvent::ToolStarted { tool_call_id, name } => {
                            TurnEvent::ToolCall { tool_call_id, name }
                        }
                        ToolExecutionEvent::ToolProgress {
                            tool_call_id,
                            name,
                            message,
                            data,
                        } => TurnEvent::ToolProgress {
                            tool_call_id,
                            name,
                            message,
                            data,
                        },
                        ToolExecutionEvent::ToolCompleted {
                            tool_call_id,
                            name,
                            is_error,
                        } => TurnEvent::ToolCompleted {
                            tool_call_id,
                            name,
                            is_error,
                        },
                        ToolExecutionEvent::ApprovalRequested {
                            tool_call_id,
                            name,
                            reason,
                        } => TurnEvent::ApprovalRequested {
                            tool_call_id,
                            name,
                            reason,
                        },
                        ToolExecutionEvent::ApprovalResolved {
                            tool_call_id,
                            name,
                            decision,
                        } => TurnEvent::ApprovalResolved {
                            tool_call_id,
                            name,
                            decision,
                        },
                    };
                    events.push(turn_event);
                }
                ToolExecutionStreamItem::Result(result) => {
                    tool_results.push(result);
                }
            }
        }

        for result in &tool_results {
            self.metrics.add_tool_call();
            if result.is_error {
                self.metrics.add_tool_error();
            }
        }

        let tool_message = Message::tool_results(tool_results);
        let compaction_config = CompactionConfig {
            max_tool_result_chars: self.config.max_tool_result_chars,
        };
        let compaction = compact_tool_result_message(tool_message, &compaction_config);
        if compaction.compacted {
            events.push(TurnEvent::CompactionStarted {
                reason: "tool_result_budget".into(),
            });
            events.push(TurnEvent::CompactionCompleted {
                reason: "tool_result_budget".into(),
            });
        }

        Ok((compaction.message, events))
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
                if iterations >= self.config.max_iterations {
                    Err(AgentError::MaxIterations(self.config.max_iterations))?;
                }
                iterations += 1;
                self.metrics.add_iteration();

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

                match self.run_compaction_phase(provider, iterations).await? {
                    CompactionResult::Continue { events, compactions } => {
                        for event in events {
                            yield event;
                        }
                        for _ in 0..compactions {
                            self.metrics.add_compaction();
                        }
                    }
                    CompactionResult::AbortTurn { events } => {
                        for event in events {
                            yield event;
                        }
                        yield TurnEvent::TurnFinished {
                            stop_reason: StopReason::EndTurn,
                            final_text: String::new(),
                        };
                        break;
                    }
                }

                let (assistant_message, stop_reason, usage, provider_events) =
                    self.call_provider(provider, &tool_definitions).await?;
                for event in provider_events {
                    yield event;
                }

                if let Some(TokenUsage { input_tokens, output_tokens }) = usage {
                    self.metrics.add_input_tokens(input_tokens);
                    self.metrics.add_output_tokens(output_tokens);
                    debug!(input_tokens, output_tokens, "provider usage");
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

                let post_events = self
                    .run_hook_phase(HookPhase::PostSampling, &hook_context, &assistant_message)
                    .await?;
                for event in post_events {
                    yield event;
                }

                let tool_calls = assistant_message.tool_calls().cloned().collect::<Vec<_>>();
                if tool_calls.is_empty() {
                    let stop_events = self
                        .run_hook_phase(HookPhase::Stop, &hook_context, &assistant_message)
                        .await?;
                    for event in stop_events {
                        yield event;
                    }

                    yield TurnEvent::TurnFinished {
                        stop_reason,
                        final_text: assistant_message.text_content(),
                    };
                    info!(?stop_reason, "turn finished");
                    break;
                }

                if self.config.cancelled.load(Ordering::Relaxed) {
                    Err(AgentError::Cancelled)?;
                }

                let (tool_message, tool_events) =
                    self.execute_tool_calls_phase(tools, tool_calls, turn_id).await?;
                for event in tool_events {
                    yield event;
                }

                self.messages.push(tool_message.clone());
                yield TurnEvent::ToolResult(tool_message);
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
        // Snapshot session state so we can roll it back if the turn errors
        // out part-way through.
        let messages_before = self.messages.clone();
        // next_turn_id is incremented inside run_turn_stream — capture it before.
        let turn_id_before = self.next_turn_id;
        let metrics_checkpoint = self.metrics.checkpoint();
        // File-read state is mutated by filesystem tools; restore it on failure
        // so stale-write protection remains consistent with the pre-turn snapshot.
        let read_file_state_before = self.read_file_state.lock().await.clone();
        let turn_result: Result<(Vec<TurnEvent>, Option<Message>, StopReason), AgentError> = {
            let mut stream = Box::pin(self.run_turn_stream(provider, tools, user_input));
            let mut events = Vec::new();
            let mut final_message = None;
            let mut stop_reason = StopReason::EndTurn;
            // Assistant messages emitted while a hook is running belong to the
            // hook, not the model. We only want the model's own final answer
            // in `TurnResult.final_message`.
            let mut in_hook_phase = false;

            while let Some(event) = stream.next().await {
                let event = event?;
                match &event {
                    TurnEvent::HookStarted { .. } => in_hook_phase = true,
                    TurnEvent::HookCompleted { .. } => in_hook_phase = false,
                    TurnEvent::IterationStarted { .. } => in_hook_phase = false,
                    TurnEvent::Assistant(message) if !in_hook_phase => {
                        final_message = Some(message.clone());
                    }
                    _ => {}
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
            Ok((events, final_message, stop_reason))
        };

        let (events, final_message, stop_reason) = match turn_result {
            Ok(result) => result,
            Err(err) => {
                self.messages = messages_before;
                self.next_turn_id = turn_id_before;
                self.metrics.restore(&metrics_checkpoint);
                *self.read_file_state.lock().await = read_file_state_before;
                return Err(err);
            }
        };

        // Persist the session if a backend is configured. A save failure should
        // not hide a successfully completed turn, so we log it and continue.
        if let Err(err) = self.save().await {
            error!(error = %err, "failed to persist session after turn");
        }

        Ok(TurnResult {
            final_message: final_message.unwrap_or_else(|| Message::assistant("")),
            events,
            stop_reason,
        })
    }
}

impl TurnEvent {
    /// Return the [`Message`] carried by this event, if any.
    ///
    /// Only [`User`](TurnEvent::User), [`Assistant`](TurnEvent::Assistant), and
    /// [`ToolResult`](TurnEvent::ToolResult) carry messages.
    pub fn message(&self) -> Option<&Message> {
        match self {
            TurnEvent::User(message)
            | TurnEvent::Assistant(message)
            | TurnEvent::ToolResult(message) => Some(message),
            _ => None,
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
            TurnEvent::ThinkingDelta { text } => format!("thinking_delta:{text}"),
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
            TurnEvent::ApprovalRequested {
                tool_call_id,
                name,
                reason,
            } => format!("approval_requested:{name}#{tool_call_id}:{reason}"),
            TurnEvent::ApprovalResolved {
                tool_call_id,
                name,
                decision,
            } => format!("approval_resolved:{name}#{tool_call_id}:{decision}"),
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
                .map(|message| {
                    message
                        .blocks
                        .iter()
                        .map(|block| match block {
                            ContentBlock::Text(text) => text.text.clone(),
                            ContentBlock::Thinking(thinking) => {
                                format!("thinking:{}", thinking.text)
                            }
                            ContentBlock::ToolCall(call) => {
                                format!("tool_call:{}({})", call.name, call.arguments)
                            }
                            ContentBlock::ToolResult(result) => {
                                format!("tool_result:{}={}", result.name, result.content)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
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
            ContentBlock::Thinking(thinking) => provider.estimate_tokens(&thinking.text),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;
    use crate::provider::{CompletionResponse, StopReason, TokenUsage};
    use crate::storage::JsonlStorage;
    use std::path::PathBuf;

    #[test]
    fn turn_event_message_returns_some_for_message_variants() {
        let message = Message::user("hi");
        assert!(matches!(TurnEvent::User(message.clone()).message(), Some(m) if m == &message));
        assert!(TurnEvent::TurnStarted {
            session_id: "s".into(),
            turn_id: 1,
            user_input: "hi".into(),
        }
        .message()
        .is_none());
    }

    #[tokio::test]
    async fn save_and_resume_restores_metadata_and_read_file_state() {
        let dir = std::env::temp_dir().join("tiny_agent_test_resume_metadata");
        let _ = std::fs::remove_dir_all(&dir);
        let storage: Arc<dyn Storage> = Arc::new(JsonlStorage::new(&dir).unwrap());

        let config = AgentConfig {
            storage: Some(Arc::clone(&storage)),
            ..Default::default()
        };

        let mut session = AgentSession::new(config.clone());
        let session_id = session.session_id().to_string();

        // Run one turn so counters advance and next_turn_id becomes 2.
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("hello"),
            stop_reason: StopReason::EndTurn,
            usage: Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        }]);
        let tools = ToolRegistry::new();
        session.run_turn(&provider, &tools, "hi").await.unwrap();
        assert_eq!(session.next_turn_id, 2);
        assert_eq!(session.metrics.turn_count(), 1);
        assert_eq!(session.metrics.total_input_tokens(), 10);
        assert_eq!(session.metrics.total_output_tokens(), 5);

        // Inject a read-file record so we can verify it round-trips.
        session
            .read_file_state
            .lock()
            .await
            .insert(PathBuf::from("src/lib.rs"), crate::tool::FileReadRecord {
                content: "fn main() {}".to_string(),
                timestamp_ms: 1234,
                is_partial_view: false,
                offset: None,
                limit: None,
            });

        session.save().await.unwrap();

        let resumed = AgentSession::resume(&session_id, config, storage)
            .await
            .unwrap();
        assert_eq!(resumed.session_id, session_id);
        assert_eq!(resumed.next_turn_id, 2);
        assert_eq!(resumed.metrics.turn_count(), 1);
        assert_eq!(resumed.metrics.total_input_tokens(), 10);
        assert_eq!(resumed.metrics.total_output_tokens(), 5);
        assert_eq!(
            resumed
                .read_file_state
                .lock()
                .await
                .get(&PathBuf::from("src/lib.rs"))
                .map(|r| r.content.as_str()),
            Some("fn main() {}")
        );
        // Messages should be restored too.
        assert_eq!(resumed.messages.len(), 2); // user + assistant (no system prompt)

        let _ = std::fs::remove_dir_all(&dir);
    }
}
