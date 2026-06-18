use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::collections::VecDeque;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use telos_agent::TurnEvent;
use tokio::sync::mpsc;

use crate::tui::approval::PendingApproval;
use crate::tui::chat_panel::ChatPanel;
use crate::tui::event::Event;
use crate::tui::input_panel::InputPanel;
use crate::tui::status_bar;
use crate::tui::theme::Theme;

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
    /// An error message (turn stream failure, session error, etc.).
    Error(String),
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
    /// Whether a background turn is currently running.
    pub turn_active: bool,
    /// Shared cancellation flag — set by Ctrl+C and read by the background task.
    cancel_flag: Arc<AtomicBool>,
    /// Send prompts to the background agent task.
    turn_tx: mpsc::UnboundedSender<String>,
    /// Receive TurnEvents from the background agent task.
    turn_rx: mpsc::UnboundedReceiver<Event>,
    /// Receive pending approvals from the TuiApprovalHandler.
    approval_rx: mpsc::UnboundedReceiver<PendingApproval>,
}

impl App {
    pub fn new(
        mut config: telos_agent::AgentConfig,
        provider: Arc<dyn telos_agent::ModelProvider>,
        tools: telos_agent::ToolRegistry,
        status_text: String,
        project_root: Option<&std::path::Path>,
    ) -> Result<Self, telos_agent::AgentError> {
        // Wire up session storage before creating the AgentSession.
        let session_manager = crate::session::SessionManager::new(project_root);
        std::fs::create_dir_all(session_manager.sessions_dir()).ok();
        let storage =
            Arc::new(telos_agent::JsonlStorage::new(session_manager.sessions_dir().to_path_buf())?);
        config.storage = Some(storage);

        // Extract the cancellation flag before moving config into the spawned task.
        let cancel_flag = Arc::clone(&config.cancelled);

        let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<String>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
        let (approval_tx, approval_rx) = mpsc::unbounded_channel::<PendingApproval>();

        // Background task owns the AgentSession because run_turn_stream needs &mut self.
        tokio::spawn(async move {
            let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> =
                Some(Arc::new(crate::tui::approval::TuiApprovalHandler::new(approval_tx)));
            let mut config = config;
            config.approval_handler = approval_handler;

            let mut session = match telos_agent::AgentSession::new(config) {
                Ok(s) => s,
                Err(e) => {
                    // Surface the error so the TUI can display it.
                    let _ = event_tx.send(Event::SessionError { message: e.to_string() });
                    let _ = event_tx.send(Event::TurnComplete);
                    return;
                }
            };

            while let Some(prompt) = prompt_rx.recv().await {
                let erased = telos_agent::ErasedProvider(provider.as_ref());
                {
                    let mut stream = pin!(session.run_turn_stream(&erased, &tools, prompt,));
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(te) => {
                                let _ = event_tx.send(Event::Turn(te));
                            }
                            Err(e) => {
                                let _ =
                                    event_tx.send(Event::SessionError { message: e.to_string() });
                                break;
                            }
                        }
                    }
                }
                let _ = session.save().await;
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
            turn_active: false,
            cancel_flag,
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

                // Global shortcuts.
                match (key.code, key.modifiers) {
                    (KeyCode::Char('d'), KeyModifiers::CONTROL) if self.input.is_empty() => {
                        self.should_quit = true;
                        return Ok(());
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        self.cancel_flag.store(true, Ordering::Relaxed);
                        self.status_text = "telos · cancelling...".to_string();
                        return Ok(());
                    }
                    (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                        self.messages.clear();
                        self.chat.scroll_to_bottom();
                        return Ok(());
                    }
                    (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                        // Full session reset requires recreating the background AgentSession,
                        // which is a follow-up enhancement. For now, clear the chat and indicate
                        // that a new session will begin on the next prompt.
                        self.messages.clear();
                        self.chat.scroll_to_bottom();
                        self.status_text = "telos · new session (next prompt)".to_string();
                        return Ok(());
                    }
                    _ => {}
                }

                match self.mode {
                    Mode::Approving => {
                        match key.code {
                            KeyCode::Char('a') | KeyCode::Char('y') => self.approve_current(),
                            KeyCode::Char('d') | KeyCode::Char('n') => {
                                self.deny_current("denied by user");
                            }
                            KeyCode::Char('e') => {
                                // Future: open editor to modify arguments.
                                self.deny_current("edit requested");
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    Mode::Normal => {
                        // Scroll keys.
                        match key.code {
                            KeyCode::PageUp => {
                                self.chat.scroll_up(10);
                                return Ok(());
                            }
                            KeyCode::PageDown => {
                                self.chat.scroll_down(10);
                                return Ok(());
                            }
                            KeyCode::Up => {
                                self.chat.scroll_up(1);
                                return Ok(());
                            }
                            KeyCode::Down => {
                                self.chat.scroll_down(1);
                                return Ok(());
                            }
                            _ => {}
                        }

                        // Input handling.
                        if let Some(prompt) = self.input.handle_key(key) {
                            self.send_prompt(prompt);
                        }
                    }
                    Mode::Streaming => {
                        // During streaming, only scroll keys are handled.
                        match key.code {
                            KeyCode::PageUp => self.chat.scroll_up(10),
                            KeyCode::PageDown => self.chat.scroll_down(10),
                            _ => {}
                        }
                    }
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
            Event::SessionError { message } => {
                self.messages.push(UiMessage::Error(message));
                self.mode = Mode::Normal;
                self.turn_active = false;
            }
            Event::Resize { .. } => {}
            Event::Turn(turn_event) => self.handle_turn_event(turn_event),
            Event::TurnComplete => {
                self.messages.push(UiMessage::TurnComplete);
                self.mode = Mode::Normal;
                self.turn_active = false;
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
        self.turn_active = true;
    }

    /// Approve the current pending approval request.
    pub fn approve_current(&mut self) {
        if let Some(pending) = self.pending_approvals.pop_front() {
            let _ = pending.respond.send(telos_agent::ApprovalDecision::Allow);
        }
        if self.pending_approvals.is_empty() {
            self.mode = if self.turn_active { Mode::Streaming } else { Mode::Normal };
        }
    }

    /// Deny the current pending approval request with a reason.
    pub fn deny_current(&mut self, reason: &str) {
        if let Some(pending) = self.pending_approvals.pop_front() {
            let _ = pending
                .respond
                .send(telos_agent::ApprovalDecision::Deny { reason: reason.to_string() });
        }
        if self.pending_approvals.is_empty() {
            self.mode = if self.turn_active { Mode::Streaming } else { Mode::Normal };
        }
    }

    /// Convert an agent `TurnEvent` into a `UiMessage`.
    fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TurnStarted { .. } => {
                // User message already pushed by send_prompt — skip the duplicate.
            }
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
            TurnEvent::ToolProgress { name, message, .. } => {
                self.status_text = format!("{}: {}", name, message);
            }
            TurnEvent::TurnFinished { final_text, .. } => {
                if !final_text.is_empty() {
                    self.messages.push(UiMessage::AssistantDelta(final_text));
                }
            }
            TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => {
                self.messages.push(UiMessage::Error(format!(
                    "token budget exceeded: {used_tokens}/{max_tokens}"
                )));
            }
            TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => {
                self.status_text =
                    format!("retrying provider ({attempt}/{max_retries}, {delay_ms}ms)");
            }
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

        if self.mode == Mode::Approving
            && let Some(pending) = self.pending_approvals.front()
        {
            let area = frame.area();
            let block_area = ratatui::layout::Rect {
                x: area.x + 4,
                y: area.y + area.height / 3,
                width: area.width.saturating_sub(8),
                height: 12.min(area.height.saturating_sub(4)),
            };
            let theme = Theme::default();
            // Solid background to obscure content underneath.
            let bg = Block::default().style(Style::default().bg(Color::Black));
            let block = Block::default()
                .title(" Approval required ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.tool_pending_fg))
                .style(Style::default().bg(Color::Rgb(24, 24, 32)));
            let args = serde_json::to_string_pretty(&pending.request.arguments)
                .unwrap_or_else(|_| pending.request.arguments.to_string());
            let text = Text::from(vec![
                Line::from(vec![
                    Span::styled("Tool:   ", Style::default().fg(Color::White)),
                    Span::styled(
                        pending.request.tool_name.clone(),
                        Style::default().fg(theme.tool_pending_fg).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Reason: ", Style::default().fg(Color::White)),
                    Span::styled(&pending.request.reason, Style::default().fg(Color::Gray)),
                ]),
                Line::from(""),
                Line::from(Span::styled(&args, Style::default().fg(Color::Gray))),
                Line::from(""),
                Line::from(Span::styled(
                    "  [a/y] approve  [d/n] deny  [e] edit-request  ",
                    Style::default().fg(Color::White),
                )),
            ]);
            let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });
            frame.render_widget(bg, block_area);
            frame.render_widget(paragraph, block_area);
        }
    }
}
