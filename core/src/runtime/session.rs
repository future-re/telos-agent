//! Agent session and turn loop orchestration.

use async_stream::try_stream;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, info_span, warn};

use super::compaction_phase::CompactionResult;
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::hooks::{HookContext, HookPhase};
use crate::message::{Message, ToolResult};
use crate::metrics::SessionMetrics;
use crate::provider::{ModelHint, ModelProvider, StopReason, TokenUsage};
use crate::runtime::TurnInputReceiver;
use crate::runtime::{TurnEvent, TurnResult};
use crate::tool::FileReadState;
use crate::tool::ToolRegistry;

static NEXT_SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// An agent session that maintains conversation state across turns.
///
/// Created via [`AgentSession::new`] or [`AgentSession::resume`].
pub struct AgentSession {
    pub(super) config: AgentConfig,
    pub(super) session_id: String,
    /// Monotonic counter; incremented at the start of each turn.
    pub(super) next_turn_id: u64,
    /// Full conversation, including the optional leading system prompt.
    pub(super) messages: Vec<Message>,
    /// Shared state used by filesystem tools to reject stale writes.
    pub(super) read_file_state: FileReadState,
    /// Accumulated session-level metrics updated by the runtime.
    pub(super) metrics: SessionMetrics,
    /// Consecutive compaction failures. Used as a circuit breaker to
    /// stop retrying when the context is irrecoverably over the limit.
    pub(super) consecutive_compaction_failures: usize,
    /// System prompt rendered once from PromptAssembly and cached.
    /// Avoids re-rendering on every provider call, which would break
    /// DeepSeek's prefix caching (identical prefix → cache hit).
    pub(super) cached_system_prompt: Option<String>,
    /// Fingerprint of the last injected memory reminder, used to skip
    /// identical re-injections that only add prompt churn.
    pub(super) last_memory_injection_fingerprint: Option<u64>,
    /// Fingerprint of the last injected skill reminder, used to skip
    /// identical re-injections that only add prompt churn.
    pub(super) last_skill_injection_fingerprint: Option<u64>,
    /// Whether persistent memory changed since the last effective injection.
    pub(super) memory_state_dirty: bool,
    /// Tracks whether the current turn already injected memory context.
    pub(super) current_turn_memory_injected: bool,
    /// Tracks whether the current turn already notified about memory mutation.
    pub(super) current_turn_memory_mutation_notified: bool,
}

impl AgentSession {
    /// Start a fresh session. System prompt is constructed at turn time via
    /// PromptAssembly. If no assembly is provided, use `base_system_prompt`
    /// as a simple fallback.
    pub fn new(config: AgentConfig) -> Result<Self, AgentError> {
        config.validate()?;
        let mut messages = Vec::new();
        // System prompt is now constructed at turn time via PromptAssembly.
        // If no assembly is provided, use base_system_prompt as simple fallback.
        if config.prompt_assembly.is_none()
            && let Some(sp) = config.base_system_prompt.as_ref()
        {
            messages.push(Message::system(sp.clone()));
        }

        Ok(Self {
            config,
            session_id: new_session_id(),
            next_turn_id: 1,
            messages,
            read_file_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            metrics: SessionMetrics::new(),
            consecutive_compaction_failures: 0,
            cached_system_prompt: None,
            last_memory_injection_fingerprint: None,
            last_skill_injection_fingerprint: None,
            memory_state_dirty: false,
            current_turn_memory_injected: false,
            current_turn_memory_mutation_notified: false,
        })
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
        self.messages = self
            .messages
            .first()
            .filter(|message| message.role == crate::message::Role::System)
            .cloned()
            .into_iter()
            .collect();
        self.next_turn_id = 1;
        self.read_file_state = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        self.cached_system_prompt = None;
        self.last_memory_injection_fingerprint = None;
        self.last_skill_injection_fingerprint = None;
        self.memory_state_dirty = false;
        self.current_turn_memory_injected = false;
        self.current_turn_memory_mutation_notified = false;
    }

    pub(super) fn push_system_reminder(&mut self, reminder: crate::message::SystemReminder) {
        self.messages.push(crate::message::Message::system(reminder.render()));
    }

    /// Run one turn, yielding [`TurnEvent`]s as the turn progresses.
    ///
    /// The stream borrows `self` mutably so the conversation is updated in
    /// place as events are produced. Errors abort the stream; partially
    /// produced events up to that point are still observed by the consumer.
    ///
    /// If `config.skill_registry` is set, a [`SkillTool`](crate::tools::SkillTool)
    /// is automatically registered into a per-turn clone of the supplied
    /// `tools` registry so the model can invoke bundled skills without the
    /// caller having to register the tool manually.
    pub fn run_turn_stream<'a, P: ModelProvider + 'a>(
        &'a mut self,
        provider: &'a P,
        tools: &'a ToolRegistry,
        user_input: impl Into<String> + 'a,
    ) -> impl Stream<Item = Result<TurnEvent, AgentError>> + 'a {
        self.run_turn_stream_with_input(
            provider,
            tools,
            user_input,
            crate::runtime::input::empty_turn_input_receiver(),
        )
    }

    /// Run one turn with a live input channel.
    ///
    /// Inputs received while tools are running are appended to the same turn
    /// after tool results and before the next provider request. The next
    /// provider request is forced to [`ModelHint::Thinking`] so routed
    /// providers can use a stronger model for reconsideration.
    pub fn run_turn_stream_with_input<'a, P: ModelProvider + 'a>(
        &'a mut self,
        provider: &'a P,
        tools: &'a ToolRegistry,
        user_input: impl Into<String> + 'a,
        mut turn_input: TurnInputReceiver,
    ) -> impl Stream<Item = Result<TurnEvent, AgentError>> + 'a {
        try_stream! {
            let mut tools = tools.clone();
            if let Some(skill_registry) = self.config.skill_registry.clone() {
                crate::tools::register_skill_tool(&mut tools, skill_registry);
            }
            let tools = tools;

            let turn_id = self.next_turn_id;
            self.next_turn_id += 1;
            let user_input = user_input.into();
            self.current_turn_memory_injected = false;
            self.current_turn_memory_mutation_notified = false;

            // If no system prompt source was configured, build the default modular
            // prompt assembly from the tool registry so the model gets the full
            // telos-agent identity, style, task guidance, safety, and tool
            // usage instructions.
            if self.config.prompt_assembly.is_none() && self.config.base_system_prompt.is_none() {
                self.config.prompt_assembly = Some(Arc::new(
                    crate::prompt::default_coding_assembly_for_profile(
                        Arc::new(tools.clone()),
                        self.config.cwd.clone(),
                        self.config.skill_registry.clone(),
                        self.config.path,
                        self.config.prompt_profile,
                    ),
                ));
            }

            self.repair_incomplete_tool_call_tail();

            let user_message = Message::user(user_input.clone());
            self.messages.push(user_message.clone());

            // Save a clone for memory injection later in the loop
            // (user_input is moved into TurnStarted below).
            let user_input_for_memory = user_input.clone();

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
            // Track state for model routing decisions
            let mut previous_tool_error = false;
            let mut consecutive_noop = 0usize;
            let mut force_thinking_next_iteration = false;
            loop {
                if let Some(max_iterations) = self.config.max_iterations
                    && iterations >= max_iterations
                {
                    Err(AgentError::MaxIterations(max_iterations))?;
                }
                iterations += 1;
                self.metrics.add_iteration();

                if self.config.cancellation.is_cancelled() {
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

                for message in Self::drain_turn_input(&mut turn_input) {
                    self.messages.push(message.clone());
                    force_thinking_next_iteration = true;
                    yield TurnEvent::User(message);
                }

                // Dynamic memory injection: score cached memories against
                // the user's current input and inject the top-K as a system
                // reminder before the first provider call of each turn.
                if iterations == 1
                    && let Some(injector) = &self.config.memory_injector
                    && let Some(injection) = injector.inject_for_query(&user_input_for_memory)
                {
                    let unchanged = self.last_memory_injection_fingerprint == Some(injection.fingerprint);
                    if self.memory_state_dirty || !unchanged {
                        debug!(
                            session_id = %self.session_id,
                            turn_id,
                            fingerprint = injection.fingerprint,
                            memory_state_dirty = self.memory_state_dirty,
                            "injecting memory reminder"
                        );
                        self.push_system_reminder(injection.reminder);
                        self.last_memory_injection_fingerprint = Some(injection.fingerprint);
                        self.current_turn_memory_injected = true;
                    } else {
                        debug!(
                            session_id = %self.session_id,
                            turn_id,
                            fingerprint = injection.fingerprint,
                            "skipping unchanged memory reminder"
                        );
                    }
                    self.memory_state_dirty = false;
                }

                if iterations == 1
                    && self.config.prompt_profile == crate::prompt::PromptProfile::Minimal
                    && let Some(injector) = &self.config.skill_injector
                    && let Some(injection) = injector.inject_for_query(&user_input_for_memory)
                {
                    let unchanged =
                        self.last_skill_injection_fingerprint == Some(injection.fingerprint);
                    if !unchanged {
                        debug!(
                            session_id = %self.session_id,
                            turn_id,
                            fingerprint = injection.fingerprint,
                            "injecting skill discovery reminder"
                        );
                        self.push_system_reminder(injection.reminder);
                        self.last_skill_injection_fingerprint = Some(injection.fingerprint);
                    } else {
                        debug!(
                            session_id = %self.session_id,
                            turn_id,
                            fingerprint = injection.fingerprint,
                            "skipping unchanged skill discovery reminder"
                        );
                    }
                }

                let hint = if force_thinking_next_iteration {
                    force_thinking_next_iteration = false;
                    ModelHint::Thinking
                } else {
                    Self::resolve_hint(
                        &self.config,
                        iterations,
                        previous_tool_error,
                        consecutive_noop,
                    )
                };

                let (assistant_message, stop_reason, usage, actual_model, provider_events) =
                    self.call_provider(provider, &tool_definitions, hint).await?;
                for event in provider_events {
                    yield event;
                }

                if let Some(TokenUsage { input_tokens, output_tokens, total_tokens, prompt_cache_hit_tokens, prompt_cache_miss_tokens, reasoning_tokens }) = usage {
                    self.metrics.add_input_tokens(input_tokens);
                    self.metrics.add_output_tokens(output_tokens);
                    if let Some(hit) = prompt_cache_hit_tokens {
                        self.metrics.add_prompt_cache_hit_tokens(hit);
                    }
                    if let Some(miss) = prompt_cache_miss_tokens {
                        self.metrics.add_prompt_cache_miss_tokens(miss);
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
                    yield TurnEvent::ProviderUsage {
                        input_tokens,
                        output_tokens,
                        total_tokens,
                        prompt_cache_hit_tokens,
                        prompt_cache_miss_tokens,
                        reasoning_tokens,
                        model: actual_model,
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

                // Track no-progress loops: tool calls but no text output
                if !tool_calls.is_empty() && assistant_message.text_content().is_empty() {
                    consecutive_noop += 1;
                } else if !tool_calls.is_empty() {
                    consecutive_noop = 0;
                }

                if tool_calls.is_empty() {
                    let mut reconsideration_inputs = Vec::new();
                    for message in Self::drain_turn_input(&mut turn_input) {
                        self.messages.push(message.clone());
                        reconsideration_inputs.push(message);
                    }
                    if !reconsideration_inputs.is_empty() {
                        for message in reconsideration_inputs {
                            yield TurnEvent::User(message);
                        }
                        force_thinking_next_iteration = true;
                        continue;
                    }

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

                if self.config.cancellation.is_cancelled() {
                    Err(AgentError::Cancelled)?;
                }

                let (tool_message, tool_events) =
                    self.execute_tool_calls_phase(&tools, tool_calls, turn_id).await?;
                for event in tool_events {
                    yield event;
                }

                // Update routing state from tool results
                previous_tool_error = tool_message.tool_results_iter().any(|r| r.is_error);

                self.messages.push(tool_message.clone());
                yield TurnEvent::ToolResult(tool_message);

                for message in Self::drain_turn_input(&mut turn_input) {
                    self.messages.push(message.clone());
                    force_thinking_next_iteration = true;
                    yield TurnEvent::User(message);
                }
            }
        }
    }

    fn drain_turn_input(turn_input: &mut TurnInputReceiver) -> Vec<Message> {
        let mut messages = Vec::new();
        while let Ok(input) = turn_input.try_recv() {
            let input = input.trim().to_string();
            if !input.is_empty() {
                messages.push(Message::user(input));
            }
        }
        messages
    }

    fn repair_incomplete_tool_call_tail(&mut self) {
        let Some(last_message) = self.messages.last() else {
            return;
        };
        if last_message.role != crate::message::Role::Assistant {
            return;
        }

        let tool_results = last_message
            .tool_calls()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: serde_json::json!({
                    "error": {
                        "kind": "cancelled",
                        "message": "Tool execution was interrupted before a result was recorded."
                    }
                }),
                is_error: true,
            })
            .collect::<Vec<_>>();
        if !tool_results.is_empty() {
            self.messages.push(Message::tool_results(tool_results));
        }
    }
    /// Run one turn to completion and return the collected events plus the
    /// final message. Persists the session to [`Storage`](crate::Storage) (if configured)
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
                if let TurnEvent::TurnFinished { stop_reason: reason, .. } = event.clone() {
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
        // not hide a successfully completed turn, so we surface the error in the
        // result while still returning the turn output to the caller.
        let save_error = match self.save().await {
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
}

fn new_session_id() -> String {
    let timestamp_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let process_id = std::process::id();
    let sequence = NEXT_SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);

    format!("session-{timestamp_ns}-{process_id}-{sequence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TaskPath;
    use crate::message::{ContentBlock, Role, SystemReminder, ToolCall};
    use crate::mock::MockProvider;
    use crate::provider::{CompletionResponse, ModelHint, StopReason, TokenUsage};
    use crate::storage::{JsonlStorage, Storage};
    use crate::tool::ToolRegistry;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn new_sessions_have_storage_safe_restart_resistant_ids() {
        let first = AgentSession::new(AgentConfig::default()).unwrap();
        let second = AgentSession::new(AgentConfig::default()).unwrap();

        for session_id in [first.session_id(), second.session_id()] {
            assert!(session_id.starts_with("session-"));
            assert!(session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            assert!(
                session_id.split('-').count() >= 4,
                "session id should include restart-resistant components: {session_id}"
            );
        }
        assert_ne!(first.session_id(), second.session_id());
    }

    #[tokio::test]
    async fn save_and_resume_restores_metadata_and_read_file_state() {
        let dir = std::env::temp_dir().join("tiny_agent_test_resume_metadata");
        let _ = std::fs::remove_dir_all(&dir);
        let storage: Arc<dyn Storage> = Arc::new(JsonlStorage::new(&dir).unwrap());

        let config = AgentConfig { storage: Some(Arc::clone(&storage)), ..Default::default() };

        let mut session = AgentSession::new(config.clone()).unwrap();
        let session_id = session.session_id().to_string();

        // Run one turn so counters advance and next_turn_id becomes 2.
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("hello"),
            stop_reason: StopReason::EndTurn,
            usage: Some(TokenUsage::new(10, 5)),
            model: None,
        }]);
        let tools = ToolRegistry::new();
        session.run_turn(&provider, &tools, "hi").await.unwrap();
        assert_eq!(session.next_turn_id, 2);
        assert_eq!(session.metrics.turn_count(), 1);
        assert_eq!(session.metrics.total_input_tokens(), 10);
        assert_eq!(session.metrics.total_output_tokens(), 5);
        assert_eq!(session.metrics.total_prompt_cache_hit_tokens(), 0);
        assert_eq!(session.metrics.total_prompt_cache_miss_tokens(), 0);

        // Inject a read-file record so we can verify it round-trips.
        session.read_file_state.lock().await.insert(
            PathBuf::from("src/lib.rs"),
            crate::tool::FileReadRecord {
                content: "fn main() {}".to_string(),
                timestamp_ms: 1234,
                is_partial_view: false,
                offset: None,
                limit: None,
            },
        );

        session.save().await.unwrap();

        let resumed = AgentSession::resume(&session_id, config, storage).await.unwrap();
        assert_eq!(resumed.session_id, session_id);
        assert_eq!(resumed.next_turn_id, 2);
        assert_eq!(resumed.metrics.turn_count(), 1);
        assert_eq!(resumed.metrics.total_input_tokens(), 10);
        assert_eq!(resumed.metrics.total_output_tokens(), 5);
        assert_eq!(resumed.metrics.total_prompt_cache_hit_tokens(), 0);
        assert_eq!(resumed.metrics.total_prompt_cache_miss_tokens(), 0);
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

    #[tokio::test]
    async fn default_config_does_not_stop_at_thirty_iterations() {
        let mut responses = (0..31)
            .map(|idx| CompletionResponse {
                message: Message {
                    role: Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: format!("call-{idx}"),
                        name: "missing".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            })
            .collect::<Vec<_>>();
        responses.push(CompletionResponse {
            message: Message::assistant("done"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        });

        let provider = MockProvider::new(responses);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig::default()).unwrap();

        let result = session.run_turn(&provider, &tools, "keep going").await.unwrap();

        assert_eq!(result.final_message.text_content(), "done");
        let iterations = result
            .events
            .iter()
            .filter(|event| matches!(event, TurnEvent::IterationStarted { .. }))
            .count();
        assert_eq!(iterations, 32);
    }

    #[tokio::test]
    async fn explicit_max_iterations_still_stops_the_turn() {
        let responses = (0..2)
            .map(|idx| CompletionResponse {
                message: Message {
                    role: Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: format!("call-{idx}"),
                        name: "missing".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            })
            .collect::<Vec<_>>();

        let provider = MockProvider::new(responses);
        let tools = ToolRegistry::new();
        let mut session =
            AgentSession::new(AgentConfig { max_iterations: Some(2), ..AgentConfig::default() })
                .unwrap();

        let err = session.run_turn(&provider, &tools, "cap this").await.unwrap_err();

        assert!(matches!(err, AgentError::MaxIterations(2)));
    }

    #[test]
    fn resolve_hint_first_iteration_is_thinking() {
        let config = AgentConfig::default(); // TaskPath::Standard
        let hint = AgentSession::resolve_hint(&config, 1, false, 0);
        assert_eq!(hint, ModelHint::Thinking);
    }

    #[test]
    fn resolve_hint_tool_error_is_recovery() {
        let config = AgentConfig::default();
        let hint = AgentSession::resolve_hint(&config, 2, true, 0);
        assert_eq!(hint, ModelHint::Recovery);
    }

    #[test]
    fn resolve_hint_execution_default() {
        let config = AgentConfig::default();
        let hint = AgentSession::resolve_hint(&config, 2, false, 0);
        assert_eq!(hint, ModelHint::Execution);
    }

    #[test]
    fn resolve_hint_fast_path_always_execution() {
        let config = AgentConfig::default().with_path(TaskPath::Fast);
        assert_eq!(AgentSession::resolve_hint(&config, 1, false, 0), ModelHint::Execution);
        assert_eq!(AgentSession::resolve_hint(&config, 2, true, 0), ModelHint::Execution);
        assert_eq!(AgentSession::resolve_hint(&config, 5, false, 3), ModelHint::Execution);
    }

    #[test]
    fn resolve_hint_stuck_detection() {
        let config = AgentConfig::default();
        let hint = AgentSession::resolve_hint(&config, 5, false, 3);
        assert_eq!(hint, ModelHint::Thinking);
    }

    #[test]
    fn resolve_hint_heavy_periodic_rethink() {
        let config = AgentConfig::default().with_path(TaskPath::Heavy);
        assert_eq!(AgentSession::resolve_hint(&config, 1, false, 0), ModelHint::Thinking);
        assert_eq!(AgentSession::resolve_hint(&config, 2, false, 0), ModelHint::Execution);
        assert_eq!(AgentSession::resolve_hint(&config, 4, false, 0), ModelHint::Thinking);
    }

    #[test]
    fn push_system_reminder_appends_system_role_message() {
        let mut session = AgentSession::new(AgentConfig::default()).unwrap();

        session.push_system_reminder(SystemReminder::ProviderContext);

        let reminder = session.messages.last().expect("reminder message should exist");
        assert_eq!(reminder.role, Role::System);
        assert!(reminder.text_content().contains("provider/model context has changed"));
    }

    #[test]
    fn reset_preserves_first_system_message_and_clears_memory_state() {
        let mut session = AgentSession::new(AgentConfig {
            base_system_prompt: Some("base prompt".into()),
            ..AgentConfig::default()
        })
        .unwrap();
        session.messages.push(Message::user("user message"));
        session.messages.push(Message::assistant("assistant message"));
        session.last_memory_injection_fingerprint = Some(42);
        session.memory_state_dirty = true;
        session.current_turn_memory_injected = true;
        session.current_turn_memory_mutation_notified = true;
        session.cached_system_prompt = Some("cached prompt".into());
        session.next_turn_id = 7;

        session.reset();

        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].role, Role::System);
        assert_eq!(session.messages[0].text_content(), "base prompt");
        assert_eq!(session.next_turn_id, 1);
        assert!(session.cached_system_prompt.is_none());
        assert!(session.last_memory_injection_fingerprint.is_none());
        assert!(!session.memory_state_dirty);
        assert!(!session.current_turn_memory_injected);
        assert!(!session.current_turn_memory_mutation_notified);
    }
}
