//! Public agent runtime facade.

mod pass;
mod session;
mod state;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures_core::Stream;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::agent::context::Conversation;
use crate::agent::policies::{PolicyContext, PolicyDecision, SessionMode};
use crate::agent::turn::{TurnEvent, TurnInputSender, TurnResult, turn_input_channel};
use crate::config::{AgentConfig, CancellationState};
use crate::error::AgentError;
use crate::model::message::Message;
use crate::model::provider::{ErasedProvider, ModelProvider};
use crate::tools::api::ToolRegistry;

use self::pass::runner::run_turn;
use self::session::SessionInfo;
use self::state::RuntimeState;

/// Provider and tool dependencies shared by agent sessions.
#[derive(Clone)]
pub struct AgentRuntime {
    config: AgentConfig,
    provider: Arc<dyn ModelProvider>,
    tools: Arc<ToolRegistry>,
}

/// A concurrency-safe conversation managed by [`AgentRuntime`].
#[derive(Clone)]
pub struct AgentSession {
    session_id: Arc<str>,
    busy: Arc<AtomicBool>,
    inner: Arc<Mutex<SessionData>>,
}

struct SessionData {
    info: SessionInfo,
    conversation: Conversation,
    state: RuntimeState,
}

/// Live event stream and completion handle for one turn.
pub struct TurnHandle {
    events: mpsc::UnboundedReceiver<TurnEvent>,
    result: Option<oneshot::Receiver<Result<TurnResult, AgentError>>>,
    input: TurnInputSender,
    cancellation: CancellationState,
    completed: bool,
}

impl AgentRuntime {
    pub fn new(
        config: AgentConfig,
        provider: Arc<dyn ModelProvider>,
        tools: ToolRegistry,
    ) -> Result<Self, AgentError> {
        config.validate()?;
        Ok(Self { config, provider, tools: Arc::new(tools) })
    }

    pub async fn create_session(&self) -> Result<AgentSession, AgentError> {
        let info = SessionInfo::new(self.config.clone())?;
        let session_id: Arc<str> = Arc::from(info.session_id());
        let mut conversation = Conversation::new();
        conversation.initial_messages(&self.config);
        run_session_policies(&info, &mut conversation, SessionMode::Create).await?;
        let state = RuntimeState::new();
        session::persistence::save(
            info.session_id(),
            info.config(),
            conversation.messages(),
            state.metrics(),
            state.read_file_state(),
            info.next_turn_id(),
        )
        .await?;
        Ok(AgentSession {
            session_id,
            busy: Arc::new(AtomicBool::new(false)),
            inner: Arc::new(Mutex::new(SessionData { info, conversation, state })),
        })
    }

    pub async fn resume_session(
        &self,
        session_id: impl Into<String>,
    ) -> Result<AgentSession, AgentError> {
        let storage =
            self.config.storage.clone().ok_or_else(|| {
                AgentError::Config("cannot resume without configured storage".into())
            })?;
        let (info, conversation, state) =
            session::persistence::resume(session_id, self.config.clone(), storage).await?;
        let mut conversation = conversation;
        run_session_policies(&info, &mut conversation, SessionMode::Resume).await?;
        session::persistence::save(
            info.session_id(),
            info.config(),
            conversation.messages(),
            state.metrics(),
            state.read_file_state(),
            info.next_turn_id(),
        )
        .await?;
        let session_id: Arc<str> = Arc::from(info.session_id());
        Ok(AgentSession {
            session_id,
            busy: Arc::new(AtomicBool::new(false)),
            inner: Arc::new(Mutex::new(SessionData { info, conversation, state })),
        })
    }

    pub fn start_turn(
        &self,
        session: &AgentSession,
        input: impl Into<String>,
    ) -> Result<TurnHandle, AgentError> {
        tokio::runtime::Handle::try_current()
            .map_err(|_| AgentError::Config("start_turn requires a Tokio runtime".into()))?;
        if session.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err()
        {
            return Err(AgentError::SessionBusy);
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (result_tx, result_rx) = oneshot::channel();
        let (input_tx, input_rx) = turn_input_channel();
        let cancellation = CancellationState::new();
        let worker_cancellation = cancellation.clone();
        let session = session.clone();
        let provider = Arc::clone(&self.provider);
        let tools = Arc::clone(&self.tools);
        let input = input.into();

        tokio::spawn(async move {
            let _busy = BusyGuard(session.busy.clone());
            let mut data = session.inner.lock().await;
            data.info.config_mut().cancellation = worker_cancellation;
            let event_log = Arc::new(std::sync::Mutex::new(Vec::new()));
            data.info.turn_event_sender = Some(event_tx.clone());
            data.info.turn_event_log = Some(Arc::clone(&event_log));

            let snapshot = SessionSnapshot::capture(&data).await;
            let SessionData { info, conversation, state } = &mut *data;
            let erased = ErasedProvider(provider.as_ref());
            let execution =
                run_turn(info, conversation, state, &erased, tools.as_ref(), input, input_rx).await;
            let execution = match execution {
                Ok(result) => result,
                Err(error) => {
                    snapshot.restore(&mut data).await;
                    let failed = TurnEvent::TurnFailed { error: error.to_string() };
                    data.info.emit_turn_event(&failed);
                    data.info.turn_event_sender = None;
                    data.info.turn_event_log = None;
                    let _ = result_tx.send(Err(error));
                    return;
                }
            };

            let events = event_log.lock().map(|log| log.clone()).unwrap_or_default();
            data.info.turn_event_sender = None;
            data.info.turn_event_log = None;
            let _ = result_tx.send(Ok(TurnResult {
                events,
                final_message: execution.final_message,
                stop_reason: execution.stop_reason,
            }));
        });

        Ok(TurnHandle {
            events: event_rx,
            result: Some(result_rx),
            input: input_tx,
            cancellation,
            completed: false,
        })
    }

    pub async fn run_turn(
        &self,
        session: &AgentSession,
        input: impl Into<String>,
    ) -> Result<TurnResult, AgentError> {
        self.start_turn(session, input)?.finish().await
    }
}

async fn run_session_policies(
    info: &SessionInfo,
    conversation: &mut Conversation,
    mode: SessionMode,
) -> Result<(), AgentError> {
    for policy in info.config().policies.session_start(mode) {
        let outcome = policy
            .evaluate(&PolicyContext::SessionStart {
                session_id: info.session_id().to_string(),
                mode,
                message_count: conversation.messages().len(),
            })
            .await?;
        for feedback in outcome.feedback {
            conversation.push_message(Message::user(feedback));
        }
        if let PolicyDecision::Reject { reason } = outcome.decision {
            return Err(AgentError::PermissionDenied(format!(
                "policy `{}` rejected SessionStart: {reason}",
                policy.name()
            )));
        }
    }
    Ok(())
}

impl AgentSession {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub async fn messages(&self) -> Vec<Message> {
        self.inner.lock().await.conversation.messages().to_vec()
    }

    pub async fn metrics(&self) -> crate::SessionMetrics {
        self.inner.lock().await.state.metrics().clone()
    }

    pub async fn reset(&self) -> Result<(), AgentError> {
        if self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(AgentError::SessionBusy);
        }
        let _busy = BusyGuard(self.busy.clone());
        let mut data = self.inner.lock().await;
        data.conversation.reset();
        data.info.next_turn_id = 1;
        data.state = RuntimeState::new();
        Ok(())
    }
}

impl TurnHandle {
    pub fn input_sender(&self) -> TurnInputSender {
        self.input.clone()
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub async fn finish(mut self) -> Result<TurnResult, AgentError> {
        while self.events.recv().await.is_some() {}
        let result = self
            .result
            .take()
            .expect("turn result receiver is present")
            .await
            .map_err(|_| AgentError::Cancelled)?;
        self.completed = true;
        result
    }
}

impl Stream for TurnHandle {
    type Item = TurnEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.events).poll_recv(cx)
    }
}

impl Drop for TurnHandle {
    fn drop(&mut self) {
        if !self.completed {
            self.cancellation.cancel();
        }
    }
}

struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct SessionSnapshot {
    config: AgentConfig,
    next_turn_id: u64,
    messages: Vec<Message>,
    metrics: crate::metrics::MetricsCheckpoint,
    read_file_state:
        std::collections::HashMap<std::path::PathBuf, crate::tools::api::FileReadRecord>,
    compaction_failures: usize,
}

impl SessionSnapshot {
    async fn capture(data: &SessionData) -> Self {
        Self {
            config: data.info.config().clone(),
            next_turn_id: data.info.next_turn_id(),
            messages: data.conversation.messages().to_vec(),
            metrics: data.state.metrics().checkpoint(),
            read_file_state: data.state.read_file_state().lock().await.clone(),
            compaction_failures: data.state.compaction_failures(),
        }
    }

    async fn restore(self, data: &mut SessionData) {
        *data.info.config_mut() = self.config;
        data.info.next_turn_id = self.next_turn_id;
        *data.conversation.messages_mut() = self.messages;
        data.state.metrics().restore(&self.metrics);
        *data.state.read_file_state().lock().await = self.read_file_state;
        data.state.set_compaction_failures(self.compaction_failures);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::policies::{
        Policy, PolicyContext, PolicyDecision, PolicyEntry, PolicyOutcome, PolicyPoint,
        PolicyRegistry,
    };
    use crate::model::message::{ContentBlock, Role, ToolCall};
    use crate::model::mock::MockProvider;
    use crate::model::provider::{
        CompletionRequest, CompletionResponse, ProviderEvent, StopReason,
    };
    use crate::storage::Storage;
    use crate::tools::api::{Tool, ToolContext, ToolDefinition, ToolOutput};
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use std::sync::atomic::AtomicUsize;

    fn runtime_with_response(text: &str) -> AgentRuntime {
        let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
            message: Message::assistant(text),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]));
        AgentRuntime::new(AgentConfig::default(), provider, ToolRegistry::new()).unwrap()
    }

    struct SessionFeedback;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "Echo".into(),
                description: "echo".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }
        }
        async fn invoke(
            &self,
            _: serde_json::Value,
            _: ToolContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    struct OneShotPassFeedback(AtomicBool);

    #[async_trait]
    impl Policy for OneShotPassFeedback {
        fn name(&self) -> &str {
            "pass-feedback"
        }

        async fn evaluate(&self, _: &PolicyContext) -> Result<PolicyOutcome, AgentError> {
            if self.0.swap(true, Ordering::SeqCst) {
                Ok(PolicyOutcome::continue_())
            } else {
                Ok(PolicyOutcome {
                    decision: PolicyDecision::Continue,
                    feedback: vec!["revise".into()],
                })
            }
        }
    }

    #[async_trait]
    impl Policy for SessionFeedback {
        fn name(&self) -> &str {
            "session-feedback"
        }
        async fn evaluate(&self, context: &PolicyContext) -> Result<PolicyOutcome, AgentError> {
            assert!(matches!(
                context,
                PolicyContext::SessionStart { mode: SessionMode::Create, .. }
            ));
            Ok(PolicyOutcome {
                decision: PolicyDecision::Continue,
                feedback: vec!["session context".into()],
            })
        }
    }

    #[tokio::test]
    async fn create_session_runs_session_start_policies() {
        let mut registry = PolicyRegistry::new();
        registry.register(PolicyEntry {
            point: PolicyPoint::SessionStart { mode: Some(SessionMode::Create) },
            policy: Arc::new(SessionFeedback),
        });
        let mut config = AgentConfig::default();
        config.policies = Arc::new(registry);
        let runtime =
            AgentRuntime::new(config, Arc::new(MockProvider::new(Vec::new())), ToolRegistry::new())
                .unwrap();
        let session = runtime.create_session().await.unwrap();
        assert_eq!(session.messages().await.last().unwrap().text_content(), "session context");
    }

    #[tokio::test]
    async fn pass_feedback_triggers_another_model_iteration() {
        let mut registry = PolicyRegistry::new();
        registry.register(PolicyEntry {
            point: PolicyPoint::TurnBeforeFinish,
            policy: Arc::new(OneShotPassFeedback(AtomicBool::new(false))),
        });
        let mut config = AgentConfig::default();
        config.policies = Arc::new(registry);
        let provider = Arc::new(MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("first"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("revised"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]));
        let runtime = AgentRuntime::new(config, provider, ToolRegistry::new()).unwrap();
        let session = runtime.create_session().await.unwrap();
        let result = runtime.run_turn(&session, "hello").await.unwrap();
        assert_eq!(result.final_message.text_content(), "revised");
        assert!(session.messages().await.iter().any(|message| message.text_content() == "revise"));
    }

    #[tokio::test]
    async fn model_policy_feedback_waits_until_tool_results_are_committed() {
        let mut registry = PolicyRegistry::new();
        registry.register(PolicyEntry {
            point: PolicyPoint::ModelResponse,
            policy: Arc::new(OneShotPassFeedback(AtomicBool::new(false))),
        });
        let mut config = AgentConfig::default();
        config.policies = Arc::new(registry);
        let provider = Arc::new(MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Echo".into(),
                        arguments: serde_json::json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(EchoTool);
        let runtime = AgentRuntime::new(config, provider, tools).unwrap();
        let session = runtime.create_session().await.unwrap();
        runtime.run_turn(&session, "hello").await.unwrap();
        let messages = session.messages().await;
        let tool_index = messages.iter().position(|message| message.role == Role::Tool).unwrap();
        let feedback_index =
            messages.iter().position(|message| message.text_content() == "revise").unwrap();
        assert!(tool_index < feedback_index);
    }

    #[tokio::test]
    async fn run_turn_commits_messages_and_returns_events() {
        let runtime = runtime_with_response("done");
        let session = runtime.create_session().await.unwrap();
        let result = runtime.run_turn(&session, "hello").await.unwrap();
        assert_eq!(result.final_message.text_content(), "done");
        assert!(result.events.iter().any(|event| matches!(event, TurnEvent::TurnFinished { .. })));
        assert!(matches!(result.events.last(), Some(TurnEvent::TurnFinished { .. })));
        assert_eq!(session.messages().await.last().unwrap().text_content(), "done");
    }

    #[tokio::test]
    async fn rejects_a_second_concurrent_turn() {
        let runtime = runtime_with_response("done");
        let session = runtime.create_session().await.unwrap();
        let first = runtime.start_turn(&session, "first").unwrap();
        assert!(matches!(runtime.start_turn(&session, "second"), Err(AgentError::SessionBusy)));
        first.finish().await.unwrap();
    }

    struct ControlledProvider {
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl ModelProvider for ControlledProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, AgentError> {
            unreachable!("the controlled provider uses its stream implementation")
        }

        fn stream_complete<'a>(
            &'a self,
            _request: CompletionRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
            Box::pin(async_stream::try_stream! {
                yield ProviderEvent::MessageStart;
                yield ProviderEvent::TextDelta("partial".into());
                self.release.notified().await;
                yield ProviderEvent::MessageStop {
                    stop_reason: StopReason::EndTurn,
                    usage: None,
                    model: None,
                };
            })
        }
    }

    #[tokio::test]
    async fn provider_delta_is_visible_before_provider_finishes() {
        let release = Arc::new(tokio::sync::Notify::new());
        let runtime = AgentRuntime::new(
            AgentConfig::default(),
            Arc::new(ControlledProvider { release: Arc::clone(&release) }),
            ToolRegistry::new(),
        )
        .unwrap();
        let session = runtime.create_session().await.unwrap();
        let mut handle = runtime.start_turn(&session, "hello").unwrap();

        loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), handle.next())
                .await
                .expect("delta should arrive while provider is blocked")
                .expect("event stream should remain open");
            if matches!(event, TurnEvent::AssistantDelta { ref text } if text == "partial") {
                break;
            }
        }
        assert!(session.is_busy());
        release.notify_waiters();
        handle.finish().await.unwrap();
    }

    #[tokio::test]
    async fn dropping_handle_cancels_and_rolls_back_session() {
        let release = Arc::new(tokio::sync::Notify::new());
        let runtime = AgentRuntime::new(
            AgentConfig::default(),
            Arc::new(ControlledProvider { release }),
            ToolRegistry::new(),
        )
        .unwrap();
        let session = runtime.create_session().await.unwrap();
        let mut handle = runtime.start_turn(&session, "temporary").unwrap();
        while let Some(event) = handle.next().await {
            if matches!(event, TurnEvent::AssistantDelta { .. }) {
                break;
            }
        }
        drop(handle);

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while session.is_busy() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled worker should release the session");
        assert!(session.messages().await.is_empty());
    }

    #[derive(Debug)]
    struct FailingStorage(AtomicUsize);

    #[async_trait]
    impl Storage for FailingStorage {
        async fn save_snapshot(
            &self,
            _session_id: &str,
            _messages: &[Message],
        ) -> Result<(), AgentError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(AgentError::Config("storage unavailable".into()))
            }
        }

        async fn append(&self, _session_id: &str, _messages: &[Message]) -> Result<(), AgentError> {
            Ok(())
        }

        async fn load(&self, _session_id: &str) -> Result<Vec<Message>, AgentError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn persistence_failure_rolls_back_turn() {
        let mut config = AgentConfig::default();
        config.storage = Some(Arc::new(FailingStorage(AtomicUsize::new(0))));
        let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("done"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]));
        let runtime = AgentRuntime::new(config, provider, ToolRegistry::new()).unwrap();
        let session = runtime.create_session().await.unwrap();
        let result = runtime.run_turn(&session, "hello").await;
        assert!(
            matches!(result, Err(AgentError::Config(message)) if message.contains("storage unavailable"))
        );
        assert!(session.messages().await.is_empty());
    }

    #[tokio::test]
    async fn resume_restores_persisted_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = AgentConfig::default();
        config.storage = Some(Arc::new(crate::storage::JsonlStorage::new(dir.path()).unwrap()));
        let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("persisted"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]));
        let runtime = AgentRuntime::new(config, provider, ToolRegistry::new()).unwrap();
        let session = runtime.create_session().await.unwrap();
        runtime.run_turn(&session, "hello").await.unwrap();

        let resumed = runtime.resume_session(session.session_id()).await.unwrap();
        assert_eq!(resumed.messages().await.last().unwrap().text_content(), "persisted");
    }
}
