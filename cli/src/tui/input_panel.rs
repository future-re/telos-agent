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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposerHints {
    left: String,
    right: Option<String>,
}

impl ComposerHints {
    fn normal(width: u16) -> Self {
        let left = String::from(" Enter send  Alt+Enter newline ");
        let right = String::from(" Ctrl+up/down history  Shift+Tab auto  Ctrl+D quit ");

        if usize::from(width) >= left.len() + right.len() + 2 {
            Self { left, right: Some(right) }
        } else {
            Self { left, right: None }
        }
    }

    fn history(index: usize, len: usize) -> Self {
        Self { left: format!(" History {}/{} ", index + 1, len), right: None }
    }
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
        textarea.set_placeholder_text("Ask tiny-agent to edit, inspect, or run...");
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

    pub fn wants_key(&self, key: KeyEvent) -> bool {
        if self.mode != InputMode::Normal {
            return true;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.history_pos.is_some() || !self.is_empty() || !self.history.is_empty()
            }
            (KeyCode::Down, KeyModifiers::NONE) => self.history_pos.is_some() || !self.is_empty(),
            (KeyCode::Up | KeyCode::Down, KeyModifiers::CONTROL) => !self.history.is_empty(),
            _ => true,
        }
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
            (KeyCode::Up, KeyModifiers::NONE)
                if (self.is_empty() || self.history_pos.is_some()) && !self.history.is_empty() =>
            {
                self.navigate_history(-1);
                InputEvent::None
            }
            (KeyCode::Down, KeyModifiers::NONE) if self.history_pos.is_some() => {
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
                // Cancel paste confirmation, keeping the pasted text for editing.
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
                    return;
                }
                if np >= len {
                    self.history_pos = None;
                    let draft = self.draft.clone();
                    self.set_text(&draft);
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
                _ => Span::styled(" Compose ", border_style),
            }
        } else {
            Span::styled(" Streaming… ", Style::default().fg(theme.thinking_fg))
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title_top(Line::from(title).left_aligned());

        if active && !matches!(self.mode, InputMode::Pasting { .. }) {
            block = block.title_top(
                Line::from(Span::styled(
                    " / commands ",
                    Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
                ))
                .right_aligned(),
            );
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Prompt prefix
        let prompt_w = 3u16;
        let prompt_area = Rect { x: inner.x, y: inner.y, width: prompt_w, height: 1 };
        let prompt = Paragraph::new(Line::from(Span::styled(
            "› ",
            Style::default().fg(theme.user_fg).add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(prompt, prompt_area);

        // Hint line
        let hints = match self.history_pos {
            Some(i) => ComposerHints::history(i, self.history.len()),
            None => ComposerHints::normal(inner.width),
        };
        let hint_style = Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM);
        let footer_y = inner.y + inner.height.saturating_sub(1);

        frame.render_widget(
            Paragraph::new(hints.left).style(hint_style),
            Rect { y: footer_y, x: inner.x, width: inner.width, height: 1 },
        );

        if let Some(right) = hints.right {
            let right_width = right.len().min(usize::from(inner.width)) as u16;
            frame.render_widget(
                Paragraph::new(right).style(hint_style),
                Rect {
                    y: footer_y,
                    x: inner.x + inner.width.saturating_sub(right_width),
                    width: right_width,
                    height: 1,
                },
            );
        }

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

#[cfg(test)]
mod tests {
    use super::{ComposerHints, InputMode, InputPanel};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn text(panel: &InputPanel) -> String {
        panel.textarea.lines().join("\n")
    }

    fn set_text(panel: &mut InputPanel, value: &str) {
        panel.set_text(value);
    }

    #[test]
    fn composer_hints_split_when_width_allows() {
        let hints = ComposerHints::normal(96);

        assert_eq!(hints.left, " Enter send  Alt+Enter newline ");
        assert_eq!(
            hints.right.as_deref(),
            Some(" Ctrl+up/down history  Shift+Tab auto  Ctrl+D quit ")
        );
    }

    #[test]
    fn composer_hints_collapse_on_narrow_width() {
        let hints = ComposerHints::normal(34);

        assert_eq!(hints.left, " Enter send  Alt+Enter newline ");
        assert_eq!(hints.right, None);
    }

    #[test]
    fn composer_hints_show_history_position() {
        let hints = ComposerHints::history(2, 5);

        assert_eq!(hints.left, " History 3/5 ");
        assert_eq!(hints.right, None);
    }

    #[test]
    fn plain_up_recalls_latest_history_when_empty() {
        let mut panel = InputPanel::new();
        panel.submit_text("first".into());
        panel.submit_text("latest".into());

        panel.handle_key(key(KeyCode::Up));

        assert_eq!(text(&panel), "latest");
    }

    #[test]
    fn plain_up_continues_to_older_history_while_browsing() {
        let mut panel = InputPanel::new();
        panel.submit_text("oldest".into());
        panel.submit_text("middle".into());
        panel.submit_text("latest".into());

        panel.handle_key(key(KeyCode::Up));
        panel.handle_key(key(KeyCode::Up));

        assert_eq!(text(&panel), "middle");
    }

    #[test]
    fn plain_down_after_history_recall_restores_empty_draft() {
        let mut panel = InputPanel::new();
        panel.submit_text("previous".into());
        panel.handle_key(key(KeyCode::Up));

        panel.handle_key(key(KeyCode::Down));

        assert_eq!(text(&panel), "");
    }

    #[test]
    fn paste_cancel_keeps_text_and_returns_to_normal_mode() {
        let mut panel = InputPanel::new();
        let pasted = [
            "This pasted text is intentionally long enough to trigger confirmation.",
            "It spans multiple lines and should stay in the composer when declined.",
            "Keeping the draft lets the user edit it instead of losing the content.",
            "The cancel action only exits confirmation mode.",
        ]
        .join("\n");
        set_text(&mut panel, &pasted);

        panel.handle_key(key(KeyCode::Enter));
        assert!(matches!(panel.input_mode(), InputMode::Pasting { .. }));

        panel.handle_key(key(KeyCode::Esc));

        assert_eq!(panel.input_mode(), InputMode::Normal);
        assert_eq!(text(&panel), pasted);
    }

    #[test]
    fn wants_plain_up_down_for_text_or_history_but_not_empty_without_history() {
        let mut panel = InputPanel::new();

        assert!(!panel.wants_key(key(KeyCode::Up)));
        assert!(!panel.wants_key(key(KeyCode::Down)));

        set_text(&mut panel, "line one\nline two");
        assert!(panel.wants_key(key(KeyCode::Up)));
        assert!(panel.wants_key(key(KeyCode::Down)));

        panel.clear_text();
        panel.submit_text("previous".into());
        assert!(panel.wants_key(key(KeyCode::Up)));
        assert!(!panel.wants_key(key(KeyCode::Down)));

        panel.handle_key(ctrl_key(KeyCode::Up));
        assert_eq!(text(&panel), "previous");
        assert!(panel.wants_key(key(KeyCode::Down)));
    }
}
