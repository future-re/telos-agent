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

        while let Some(command) = command_rx.recv().await {
            match command {
                BackgroundCommand::Prompt(prompt) => {
                    let erased = telos_agent::ErasedProvider(current_provider.as_ref());
                    {
                        let mut stream = pin!(session.run_turn_stream(&erased, &tools, prompt));
                        while let Some(event) = stream.next().await {
                            match event {
                                Ok(turn_event) => {
                                    let _ = event_tx.send(Event::Turn(turn_event));
                                }
                                Err(err) => {
                                    let _ = event_tx
                                        .send(Event::SessionError { message: err.to_string() });
                                    break;
                                }
                            }
                        }
                    }
                    let _ = session.save().await;
                    let _ = event_tx.send(Event::TurnComplete);
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
