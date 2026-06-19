use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tui_textarea::TextArea;

use crate::tui::command_popup::{CommandPopup, SlashCommand};
use crate::tui::theme::Theme;

/// What the input panel wants the app to do next.
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// User submitted a text prompt.
    Submit(String),
    /// User selected a slash command.
    SlashCommand(SlashCommand),
    /// No action (key was consumed internally).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal text input.
    Normal,
    /// User is typing a slash command.
    SlashCommand,
    /// User pasted large content — waiting for confirmation.
    Pasting { line_count: usize },
}

pub struct InputPanel {
    textarea: TextArea<'static>,
    /// Sent messages — used for up/down history navigation.
    history: Vec<String>,
    /// Current position in history (None = fresh input).
    history_pos: Option<usize>,
    /// Snapshot of current input before browsing history.
    draft: String,
    /// Current input mode.
    mode: InputMode,
    /// Slash command popup.
    pub popup: CommandPopup,
}

impl InputPanel {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea
            .set_placeholder_text("Message… (/ for commands, Enter to send, Alt+Enter newline)");
        textarea.set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));
        Self {
            textarea,
            history: Vec::new(),
            history_pos: None,
            draft: String::new(),
            mode: InputMode::Normal,
            popup: CommandPopup::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.lines().join("").trim().is_empty()
    }

    /// Current input mode.
    pub fn input_mode(&self) -> InputMode {
        self.mode
    }

    /// Process a key event. Returns an InputEvent.
    pub fn handle_key(&mut self, key: KeyEvent) -> InputEvent {
        match self.mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::SlashCommand => self.handle_slash_key(key),
            InputMode::Pasting { .. } => self.handle_paste_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> InputEvent {
        match (key.code, key.modifiers) {
            // ── Submit ──────────────────────────────────────────────
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let text = self.textarea.lines().join("\n");
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    return InputEvent::None;
                }
                // Check for paste detection
                let line_count = trimmed.lines().count();
                if line_count > 3 && trimmed.len() > 200 {
                    self.mode = InputMode::Pasting { line_count };
                    return InputEvent::None;
                }
                self.submit_text(trimmed)
            }
            // ── Newline ─────────────────────────────────────────────
            (KeyCode::Enter, KeyModifiers::ALT) => {
                self.textarea.insert_newline();
                // Check if first char on first line is '/'
                let first_line = self.textarea.lines().first().cloned().unwrap_or_default();
                if first_line == "/" && self.textarea.lines().len() == 1 {
                    self.mode = InputMode::SlashCommand;
                    self.popup.show();
                    self.textarea.move_cursor(tui_textarea::CursorMove::End);
                }
                InputEvent::None
            }
            // ── Slash command detection ─────────────────────────────
            (KeyCode::Char('/'), KeyModifiers::NONE)
                if self.textarea.lines().join("").is_empty() =>
            {
                self.textarea.insert_char('/');
                self.mode = InputMode::SlashCommand;
                self.popup.show();
                InputEvent::None
            }
            // ── History ─────────────────────────────────────────────
            (KeyCode::Up, KeyModifiers::CONTROL) if !self.history.is_empty() => {
                self.navigate_history(-1);
                InputEvent::None
            }
            (KeyCode::Down, KeyModifiers::CONTROL) if !self.history.is_empty() => {
                self.navigate_history(1);
                InputEvent::None
            }
            // ── Default ─────────────────────────────────────────────
            _ => {
                if self.history_pos.is_some() {
                    self.history_pos = None;
                }
                self.textarea.input(key);
                InputEvent::None
            }
        }
    }

    fn handle_slash_key(&mut self, key: KeyEvent) -> InputEvent {
        match key.code {
            KeyCode::Enter => {
                // Execute selected command
                if let Some(cmd) = self.popup.selected_command() {
                    let cmd = cmd.clone();
                    self.clear_text();
                    self.mode = InputMode::Normal;
                    self.popup.hide();
                    return InputEvent::SlashCommand(cmd);
                }
                InputEvent::None
            }
            KeyCode::Down | KeyCode::Tab => {
                self.popup.select_next();
                InputEvent::None
            }
            KeyCode::Up => {
                self.popup.select_prev();
                InputEvent::None
            }
            KeyCode::Esc => {
                // Exit slash command mode, keep the `/` text
                self.mode = InputMode::Normal;
                self.popup.hide();
                InputEvent::None
            }
            KeyCode::Backspace => {
                let text = self.textarea.lines().join("");
                if text.len() <= 1 {
                    // Deleting the `/` — exit slash mode
                    self.textarea.input(key);
                    self.mode = InputMode::Normal;
                    self.popup.hide();
                } else {
                    self.textarea.input(key);
                    let text = self.textarea.lines().join("");
                    self.popup.update_filter(&text[1..]); // skip '/'
                }
                InputEvent::None
            }
            _ => {
                self.textarea.input(key);
                let text = self.textarea.lines().join("");
                if text.starts_with('/') && text.len() > 1 {
                    self.popup.update_filter(&text[1..]);
                }
                InputEvent::None
            }
        }
    }

    fn handle_paste_key(&mut self, key: KeyEvent) -> InputEvent {
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                // Confirm paste — submit
                let text = self.textarea.lines().join("\n");
                let trimmed = text.trim().to_string();
                self.mode = InputMode::Normal;
                self.submit_text(trimmed)
            }
            (KeyCode::Char('n'), KeyModifiers::NONE) | (KeyCode::Esc, _) => {
                // Cancel paste — clear and return to normal
                self.clear_text();
                self.mode = InputMode::Normal;
                InputEvent::None
            }
            _ => InputEvent::None,
        }
    }

    fn submit_text(&mut self, text: String) -> InputEvent {
        self.history.push(text.clone());
        self.history_pos = None;
        self.draft.clear();
        self.clear_text();
        InputEvent::Submit(text)
    }

    fn navigate_history(&mut self, delta: isize) {
        let len = self.history.len() as isize;
        if len == 0 {
            return;
        }

        let new_pos = match self.history_pos {
            Some(p) => {
                let np = p as isize + delta;
                if np < 0 {
                    self.history_pos = None;
                    let draft = self.draft.clone();
                    self.set_text(&draft);
                    return;
                }
                if np >= len {
                    return;
                }
                np as usize
            }
            None => {
                self.draft = self.textarea.lines().join("\n");
                if delta < 0 {
                    (len - 1) as usize
                } else {
                    return;
                }
            }
        };

        self.history_pos = Some(new_pos);
        let text = self.history[new_pos].clone();
        self.set_text(&text);
    }

    fn clear_text(&mut self) {
        self.textarea.select_all();
        self.textarea.cut();
    }

    fn set_text(&mut self, text: &str) {
        self.clear_text();
        self.textarea.insert_str(text);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, active: bool) {
        let theme = Theme::default();
        let border_style = if active {
            Style::default().fg(theme.border_active).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.border_inactive)
        };

        let title = if active {
            match self.mode {
                InputMode::Pasting { line_count } => {
                    Span::styled(format!(" Pasted {line_count} lines — y(es)/n(o)? "), border_style)
                }
                _ => Span::styled(" Message ", border_style),
            }
        } else {
            Span::styled(" Streaming… ", Style::default().fg(theme.thinking_fg))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title_top(Line::from(title).left_aligned());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Prompt prefix
        let prompt_w = 2u16;
        let prompt_area = Rect { x: inner.x, y: inner.y, width: prompt_w, height: 1 };
        let prompt = Paragraph::new(Line::from(Span::styled(
            "> ",
            Style::default().fg(theme.user_fg).add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(prompt, prompt_area);

        // Hint line
        let hint = match self.history_pos {
            Some(i) => format!(" history [{}/{}] ", i + 1, self.history.len()),
            None => String::from(
                " enter·send  alt+enter·newline  ctrl+↑↓·history  shift+tab·auto  ctrl+d·quit ",
            ),
        };
        let hint_widget = Paragraph::new(hint)
            .style(Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM));
        frame.render_widget(
            hint_widget,
            Rect {
                y: inner.y + inner.height.saturating_sub(1),
                x: inner.x,
                width: inner.width,
                height: 1,
            },
        );

        // Render the textarea
        let input_area = Rect {
            x: inner.x + prompt_w,
            y: inner.y,
            width: inner.width.saturating_sub(prompt_w),
            height: inner.height.saturating_sub(1),
        };
        frame.render_widget(&self.textarea, input_area);

        // Render command popup above the input area
        if self.popup.visible {
            self.popup.render(frame, input_area, &theme);
        }
    }
}

impl Default for InputPanel {
    fn default() -> Self {
        Self::new()
    }
}
