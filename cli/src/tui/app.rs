use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::Paragraph;
use std::collections::VecDeque;
use telos_agent::TurnEvent;

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
}

impl App {
    pub fn new(status_text: String) -> Self {
        Self {
            mode: Mode::Normal,
            should_quit: false,
            status_text,
            messages: Vec::new(),
            chat: ChatPanel::new(),
            input: InputPanel::new(),
            pending_approvals: VecDeque::new(),
        }
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
            Event::Tick => {}
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

    /// Queue a user prompt as a new UI message.
    pub fn send_prompt(&mut self, prompt: String) {
        self.messages.push(UiMessage::User(prompt));
        self.mode = Mode::Streaming;
    }

    /// Convert an agent `TurnEvent` into a `UiMessage`.
    fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::AssistantDelta { text } => {
                self.messages.push(UiMessage::AssistantDelta(text));
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

        let placeholder =
            Paragraph::new("Welcome to telos TUI.\nPress Ctrl+D on empty input to exit.");
        frame.render_widget(placeholder, layout[1]);

        self.input.render(frame, layout[2], self.mode == Mode::Normal);
    }
}
