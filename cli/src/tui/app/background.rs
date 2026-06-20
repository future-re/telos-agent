use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::tui::approval::PendingApproval;
use crate::tui::event::Event;

pub(super) enum BackgroundCommand {
    Prompt(String),
    SetProvider { provider: Arc<dyn telos_agent::ModelProvider>, label: String },
    NewSession,
    ResumeSession(String),
}

enum CommandAfterTurn {
    SetProvider { provider: Arc<dyn telos_agent::ModelProvider>, label: String },
    NewSession,
    ResumeSession(String),
}

pub(super) fn spawn_background_session(
    mut config: telos_agent::AgentConfig,
    provider: Arc<dyn telos_agent::ModelProvider>,
    tools: telos_agent::ToolRegistry,
    storage: Arc<dyn telos_agent::Storage>,
    auto_mode: Arc<AtomicBool>,
    approval_tx: mpsc::UnboundedSender<PendingApproval>,
    event_tx: mpsc::UnboundedSender<Event>,
    mut command_rx: mpsc::UnboundedReceiver<BackgroundCommand>,
) {
    tokio::spawn(async move {
        let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> =
            Some(Arc::new(crate::tui::approval::TuiApprovalHandler::new(approval_tx, auto_mode)));
        config.approval_handler = approval_handler;
        let base_config = config.clone();

        let mut session = match telos_agent::AgentSession::new(config) {
            Ok(session) => session,
            Err(err) => {
                let _ = event_tx.send(Event::SessionError { message: err.to_string() });
                let _ = event_tx.send(Event::TurnComplete);
                return;
            }
        };
        let mut current_provider = provider;

        let mut deferred_commands = std::collections::VecDeque::new();

        while let Some(command) = command_rx.recv().await {
            match command {
                BackgroundCommand::Prompt(prompt) => {
                    let erased = telos_agent::ErasedProvider(current_provider.as_ref());
                    let (turn_input_tx, turn_input_rx) = telos_agent::turn_input_channel();
                    {
                        let mut stream = pin!(session.run_turn_stream_with_input(
                            &erased,
                            &tools,
                            prompt,
                            turn_input_rx
                        ));
                        let mut command_rx_closed = false;
                        loop {
                            tokio::select! {
                                event = stream.next() => {
                                    match event {
                                        Some(Ok(turn_event)) => {
                                            let _ = event_tx.send(Event::Turn(turn_event));
                                        }
                                        Some(Err(err)) => {
                                            let _ = event_tx
                                                .send(Event::SessionError { message: err.to_string() });
                                            break;
                                        }
                                        None => break,
                                    }
                                }
                                command = command_rx.recv(), if !command_rx_closed => {
                                    match command {
                                        Some(BackgroundCommand::Prompt(prompt)) => {
                                            let _ = turn_input_tx.send(prompt);
                                        }
                                        Some(BackgroundCommand::SetProvider { provider, label }) => {
                                            deferred_commands.push_back(CommandAfterTurn::SetProvider { provider, label });
                                        }
                                        Some(BackgroundCommand::NewSession) => {
                                            deferred_commands.push_back(CommandAfterTurn::NewSession);
                                        }
                                        Some(BackgroundCommand::ResumeSession(session_id)) => {
                                            deferred_commands.push_back(CommandAfterTurn::ResumeSession(session_id));
                                        }
                                        None => {
                                            command_rx_closed = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let _ = session.save().await;
                    let _ = event_tx.send(Event::TurnComplete);
                    while let Some(command) = deferred_commands.pop_front() {
                        apply_command_after_turn(
                            command,
                            &mut session,
                            &mut current_provider,
                            &base_config,
                            storage.clone(),
                            &event_tx,
                        )
                        .await;
                    }
                }
                BackgroundCommand::SetProvider { provider, label } => {
                    current_provider = provider;
                    let _ = event_tx.send(Event::SessionNotice {
                        message: format!("model switched to {label}"),
                    });
                }
                BackgroundCommand::NewSession => {
                    session = match telos_agent::AgentSession::new(base_config.clone()) {
                        Ok(session) => session,
                        Err(err) => {
                            let _ = event_tx.send(Event::SessionError { message: err.to_string() });
                            continue;
                        }
                    };
                    let _ = event_tx
                        .send(Event::SessionNotice { message: "new session started".to_string() });
                }
                BackgroundCommand::ResumeSession(session_id) => {
                    session = match telos_agent::AgentSession::resume(
                        session_id.clone(),
                        base_config.clone(),
                        storage.clone(),
                    )
                    .await
                    {
                        Ok(session) => session,
                        Err(err) => {
                            let _ = event_tx.send(Event::SessionError { message: err.to_string() });
                            continue;
                        }
                    };
                    let _ = event_tx.send(Event::SessionNotice {
                        message: format!("resumed session {session_id}"),
                    });
                }
            }
        }
    });
}

async fn apply_command_after_turn(
    command: CommandAfterTurn,
    session: &mut telos_agent::AgentSession,
    current_provider: &mut Arc<dyn telos_agent::ModelProvider>,
    base_config: &telos_agent::AgentConfig,
    storage: Arc<dyn telos_agent::Storage>,
    event_tx: &mpsc::UnboundedSender<Event>,
) {
    match command {
        CommandAfterTurn::SetProvider { provider, label } => {
            *current_provider = provider;
            let _ = event_tx
                .send(Event::SessionNotice { message: format!("model switched to {label}") });
        }
        CommandAfterTurn::NewSession => {
            *session = match telos_agent::AgentSession::new(base_config.clone()) {
                Ok(session) => session,
                Err(err) => {
                    let _ = event_tx.send(Event::SessionError { message: err.to_string() });
                    return;
                }
            };
            let _ =
                event_tx.send(Event::SessionNotice { message: "new session started".to_string() });
        }
        CommandAfterTurn::ResumeSession(session_id) => {
            *session = match telos_agent::AgentSession::resume(
                session_id.clone(),
                base_config.clone(),
                storage,
            )
            .await
            {
                Ok(session) => session,
                Err(err) => {
                    let _ = event_tx.send(Event::SessionError { message: err.to_string() });
                    return;
                }
            };
            let _ = event_tx
                .send(Event::SessionNotice { message: format!("resumed session {session_id}") });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use telos_agent::{
        AgentConfig, AgentError, CompletionResponse, ContentBlock, Message, MockProvider,
        ModelHint, NoopStorage, Role, StopReason, Tool, ToolCall, ToolContext, ToolDefinition,
        ToolOutput, ToolRegistry,
    };

    struct WaitTool {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl Tool for WaitTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "wait".into(),
                description: "Wait until the test releases the tool".into(),
                input_schema: json!({ "type": "object" }),
            }
        }

        async fn invoke(
            &self,
            _arguments: Value,
            _context: ToolContext,
        ) -> Result<ToolOutput, AgentError> {
            self.started.notify_waiters();
            self.release.notified().await;
            Ok(ToolOutput::json(json!({ "status": "released" })))
        }
    }

    #[tokio::test]
    async fn active_prompt_command_is_forwarded_into_running_turn() {
        let provider = Arc::new(MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "wait".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("rethought"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mut tools = ToolRegistry::new();
        tools.register(WaitTool { started: Arc::clone(&started), release: Arc::clone(&release) });

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (approval_tx, _approval_rx) = mpsc::unbounded_channel();
        let storage: Arc<dyn telos_agent::Storage> = Arc::new(NoopStorage);
        let provider_for_background: Arc<dyn telos_agent::ModelProvider> = provider.clone();

        spawn_background_session(
            AgentConfig::default(),
            provider_for_background,
            tools,
            storage,
            Arc::new(AtomicBool::new(false)),
            approval_tx,
            event_tx,
            command_rx,
        );

        command_tx.send(BackgroundCommand::Prompt("start".into())).unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(100), started.notified())
            .await
            .expect("tool should start");
        command_tx.send(BackgroundCommand::Prompt("new input while tool runs".into())).unwrap();
        release.notify_waiters();

        let mut saw_runtime_user_event = false;
        loop {
            let event =
                tokio::time::timeout(std::time::Duration::from_millis(500), event_rx.recv())
                    .await
                    .expect("background event should arrive")
                    .expect("event channel should stay open");
            match event {
                Event::Turn(telos_agent::TurnEvent::User(message))
                    if message.text_content() == "new input while tool runs" =>
                {
                    saw_runtime_user_event = true;
                }
                Event::TurnComplete => break,
                Event::SessionError { message } => panic!("session failed: {message}"),
                _ => {}
            }
        }

        assert!(saw_runtime_user_event);
        let requests = provider.requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].model_hint, Some(ModelHint::Thinking));
        let request_text =
            requests[1].messages.iter().map(Message::text_content).collect::<Vec<_>>().join("\n");
        assert!(request_text.contains("new input while tool runs"));
    }
}
