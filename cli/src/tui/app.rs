use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use std::collections::VecDeque;
use std::pin::pin;
use std::sync::Arc;
use telos_agent::TurnEvent;
use tokio::sync::mpsc;

use crate::tui::approval::PendingApproval;
use crate::tui::chat_panel::ChatPanel;
use crate::tui::event::Event;
use crate::tui::input_panel::InputPanel;
use crate::tui::status_bar;

/// TUI application mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Waiting for user input.
    Normal,
    /// Agent is streaming a response.
    Streaming,
    /// Approval overlay is visible.
    Approving,
}

/// A UI-facing message fragment.
#[derive(Debug, Clone)]
pub enum UiMessage {
    /// Full user prompt.
    User(String),
    /// Incremental assistant text fragment.
    AssistantDelta(String),
    /// Incremental reasoning/thinking fragment.
    ThinkingDelta(String),
    /// Tool call started.
    ToolCall { id: String, name: String },
    /// Tool call finished.
    ToolCompleted { id: String, name: String, is_error: bool },
    /// Marks the end of a turn.
    TurnComplete,
}

/// Root application state for the TUI.
pub struct App {
    /// Current UI mode.
    pub mode: Mode,
    /// Whether the application should exit.
    pub should_quit: bool,
    /// Status text shown in the top bar.
    pub status_text: String,
    /// Accumulated messages for display.
    pub messages: Vec<UiMessage>,
    /// Chat panel (rendering + scrolling).
    pub chat: ChatPanel,
    /// Input panel at the bottom.
    pub input: InputPanel,
    /// Approval requests waiting for user decision.
    pub pending_approvals: VecDeque<PendingApproval>,
    /// Send prompts to the background agent task.
    turn_tx: mpsc::UnboundedSender<String>,
    /// Receive TurnEvents from the background agent task.
    turn_rx: mpsc::UnboundedReceiver<Event>,
    /// Receive pending approvals from the TuiApprovalHandler.
    approval_rx: mpsc::UnboundedReceiver<PendingApproval>,
}

impl App {
    pub fn new(
        config: telos_agent::AgentConfig,
        provider: Arc<dyn telos_agent::ModelProvider>,
        tools: telos_agent::ToolRegistry,
        status_text: String,
    ) -> Result<Self, telos_agent::AgentError> {
        let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<String>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
        let (approval_tx, approval_rx) = mpsc::unbounded_channel::<PendingApproval>();

        // Background task owns the AgentSession because run_turn_stream needs &mut self.
        tokio::spawn(async move {
            let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> =
                Some(Arc::new(crate::tui::approval::TuiApprovalHandler::new(approval_tx)));
            let mut config = config;
            config.approval_handler = approval_handler;

            let mut session =
                telos_agent::AgentSession::new(config).expect("failed to create agent session");

            while let Some(prompt) = prompt_rx.recv().await {
                let erased = telos_agent::ErasedProvider(provider.as_ref());
                let mut stream = pin!(session.run_turn_stream(&erased, &tools, prompt,));
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(te) => {
                            let _ = event_tx.send(Event::Turn(te));
                        }
                        Err(_e) => break,
                    }
                }
                let _ = event_tx.send(Event::TurnComplete);
            }
        });

        Ok(Self {
            mode: Mode::Normal,
            should_quit: false,
            status_text,
            messages: Vec::new(),
            chat: ChatPanel::new(),
            input: InputPanel::new(),
            pending_approvals: VecDeque::new(),
            turn_tx: prompt_tx,
            turn_rx: event_rx,
            approval_rx,
        })
    }

    /// Process a single event.
    pub fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Key(key) => {
                use crossterm::event::{KeyCode, KeyModifiers};
                if key.code == KeyCode::Char('d')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.input.is_empty()
                {
                    self.should_quit = true;
                    return Ok(());
                }
                if self.mode == Mode::Normal
                    && let Some(prompt) = self.input.handle_key(key)
                {
                    self.send_prompt(prompt);
                }
            }
            Event::Tick => {
                while let Ok(event) = self.turn_rx.try_recv() {
                    self.handle_event(event)?;
                }
                while let Ok(pending) = self.approval_rx.try_recv() {
                    self.pending_approvals.push_back(pending);
                    self.mode = Mode::Approving;
                }
            }
            Event::Resize { .. } => {}
            Event::Turn(turn_event) => self.handle_turn_event(turn_event),
            Event::TurnComplete => {
                self.messages.push(UiMessage::TurnComplete);
                self.mode = Mode::Normal;
            }
            Event::Mouse(_) => {}
        }
        Ok(())
    }

    /// Send a user prompt to the background agent task.
    pub fn send_prompt(&mut self, prompt: String) {
        self.messages.push(UiMessage::User(prompt.clone()));
        let _ = self.turn_tx.send(prompt);
        self.mode = Mode::Streaming;
    }

    /// Convert an agent `TurnEvent` into a `UiMessage`.
    fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::AssistantDelta { text } => {
                self.messages.push(UiMessage::AssistantDelta(text));
                self.chat.scroll_to_bottom();
            }
            TurnEvent::ThinkingDelta { text } => {
                self.messages.push(UiMessage::ThinkingDelta(text));
            }
            TurnEvent::ToolCall { tool_call_id, name } => {
                self.messages.push(UiMessage::ToolCall { id: tool_call_id, name });
            }
            TurnEvent::ToolCompleted { tool_call_id, name, is_error } => {
                self.messages.push(UiMessage::ToolCompleted { id: tool_call_id, name, is_error });
            }
            // TODO: ignored intentionally while stubbing
            _ => {}
        }
    }

    /// Draw the entire UI.
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // status bar
                Constraint::Min(0),    // chat panel
                Constraint::Length(3), // input panel
            ])
            .split(area);

        status_bar::render(frame, layout[0], &self.status_text);

        self.chat.render(frame, layout[1], &self.messages);

        self.input.render(frame, layout[2], self.mode == Mode::Normal);
    }
}
