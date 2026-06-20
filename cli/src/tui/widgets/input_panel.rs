use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use tui_textarea::TextArea;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::command_popup::{CommandPopup, SlashCommand};
use crate::tui::keymap::is_ctrl_modifier;
use crate::tui::theme::Theme;

const PROMPT_WIDTH: u16 = 2;

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
        let right = String::from(" Shift+Tab auto  Ctrl+D quit ");

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
        textarea.set_placeholder_text("Ask tiny-agent...");
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

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn clear(&mut self) {
        self.clear_text();
        self.history_pos = None;
        self.draft.clear();
        self.mode = InputMode::Normal;
        self.popup.hide();
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_pos = None;
        self.draft.clear();
    }

    pub fn replace_history(&mut self, history: Vec<String>) {
        self.history = history.into_iter().filter(|item| !item.trim().is_empty()).collect();
        self.history_pos = None;
        self.draft.clear();
    }

    pub fn record_history(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        if self.history.last() == Some(&text) {
            return;
        }
        self.history.push(text);
        self.history_pos = None;
        self.draft.clear();
    }

    pub fn insert_text(&mut self, text: &str) {
        if self.history_pos.is_some() {
            self.history_pos = None;
        }
        self.textarea.insert_str(text);
    }

    /// Current input mode.
    pub fn input_mode(&self) -> InputMode {
        self.mode
    }

    pub fn desired_height(&self, width: usize, min_height: u16, max_height: u16) -> u16 {
        if self.is_empty() {
            return min_height;
        }
        let usable_width = composer_text_width(width as u16);
        let text_lines = self
            .textarea
            .lines()
            .iter()
            .map(|line| wrap_line(line, usable_width).len())
            .sum::<usize>()
            .max(1);
        let wanted = text_lines.saturating_add(3) as u16;
        wanted.clamp(min_height, max_height.max(min_height))
    }

    pub fn wants_vertical_nav_key(&self, key: KeyEvent) -> bool {
        if self.mode != InputMode::Normal {
            return true;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.history_pos.is_some() || !self.is_empty() || !self.history.is_empty()
            }
            (KeyCode::Down, KeyModifiers::NONE) => self.history_pos.is_some() || !self.is_empty(),
            (KeyCode::Up | KeyCode::Down, modifiers) if is_ctrl_modifier(modifiers) => {
                !self.history.is_empty()
            }
            _ => false,
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
            (KeyCode::Up, modifiers) if is_ctrl_modifier(modifiers) && !self.history.is_empty() => {
                self.navigate_history(-1);
                InputEvent::None
            }
            (KeyCode::Down, modifiers)
                if is_ctrl_modifier(modifiers) && !self.history.is_empty() =>
            {
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
        self.record_history(text.clone());
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

    pub fn restore_text(&mut self, text: String) {
        self.set_text(&text);
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

        let prompt_area = Rect { x: inner.x, y: inner.y, width: PROMPT_WIDTH, height: 1 };
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

        let input_area = Rect {
            x: inner.x + PROMPT_WIDTH,
            y: inner.y,
            width: inner.width.saturating_sub(PROMPT_WIDTH),
            height: inner.height.saturating_sub(1),
        };
        self.render_composer_text(frame, input_area, active, &theme);

        // Render command popup above the input area
        if self.popup.visible {
            self.popup.render(frame, input_area, &theme);
        }
    }

    fn render_composer_text(&self, frame: &mut Frame, area: Rect, active: bool, theme: &Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let width = usize::from(area.width).max(1);
        let (visible_lines, cursor_row, cursor_col, style) = if self.is_empty() {
            let placeholder = self.textarea.placeholder_text();
            (
                wrap_line(placeholder, width),
                0,
                0,
                self.textarea
                    .placeholder_style()
                    .unwrap_or_else(|| Style::default().fg(theme.input_placeholder)),
            )
        } else {
            let wrapped = wrap_lines(self.textarea.lines(), width);
            let (cursor_row, cursor_col) =
                wrapped_cursor(self.textarea.lines(), self.textarea.cursor(), width);
            (wrapped, cursor_row, cursor_col, self.textarea.style())
        };

        let height = usize::from(area.height);
        let top_row = cursor_row.saturating_add(1).saturating_sub(height);
        let lines = visible_lines
            .into_iter()
            .skip(top_row)
            .take(height)
            .map(|line| Line::from(Span::styled(line, style)))
            .collect::<Vec<_>>();

        frame.render_widget(Paragraph::new(Text::from(lines)), area);

        if active && !self.is_empty() {
            frame.set_cursor_position(Position {
                x: area.x + (cursor_col as u16).min(area.width.saturating_sub(1)),
                y: area.y
                    + (cursor_row.saturating_sub(top_row) as u16)
                        .min(area.height.saturating_sub(1)),
            });
        }
    }
}

fn composer_text_width(outer_width: u16) -> usize {
    usize::from(outer_width.saturating_sub(2).saturating_sub(PROMPT_WIDTH)).max(1)
}

fn wrap_lines(lines: &[String], width: usize) -> Vec<String> {
    lines.iter().flat_map(|line| wrap_line(line, width)).collect()
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut wrapped = Vec::new();
    let mut remaining = line;
    while UnicodeWidthStr::width(remaining) > width {
        let boundary = split_at_display_width(remaining, width);
        let candidate = &remaining[..boundary];
        if let Some(space_idx) = candidate.rfind(' ').filter(|idx| *idx > 0) {
            wrapped.push(remaining[..space_idx].to_string());
            remaining = remaining[space_idx + 1..].trim_start_matches(' ');
        } else {
            wrapped.push(candidate.to_string());
            remaining = &remaining[boundary..];
        }
    }
    wrapped.push(remaining.to_string());
    wrapped
}

fn split_at_display_width(input: &str, width: usize) -> usize {
    let mut display_width = 0;
    for (idx, ch) in input.char_indices() {
        let next_width = display_width + UnicodeWidthChar::width(ch).unwrap_or(0);
        if next_width > width {
            return if idx == 0 { ch.len_utf8() } else { idx };
        }
        display_width = next_width;
    }
    input.len()
}

fn wrapped_cursor(lines: &[String], cursor: (usize, usize), width: usize) -> (usize, usize) {
    let (cursor_line, cursor_col) = cursor;
    let rows_before =
        lines.iter().take(cursor_line).map(|line| wrap_line(line, width).len()).sum::<usize>();
    let prefix = lines
        .get(cursor_line)
        .map(|line| line.chars().take(cursor_col).collect::<String>())
        .unwrap_or_default();
    let wrapped_prefix = wrap_line(&prefix, width);
    let cursor_row = rows_before + wrapped_prefix.len().saturating_sub(1);
    let cursor_col =
        wrapped_prefix.last().map(|line| UnicodeWidthStr::width(line.as_str())).unwrap_or(0);
    (cursor_row, cursor_col)
}

impl Default for InputPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{ComposerHints, InputMode, InputPanel, SlashCommand};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn text(panel: &InputPanel) -> String {
        panel.textarea.lines().join("\n")
    }

    fn set_text(panel: &mut InputPanel, value: &str) {
        panel.set_text(value);
    }

    fn rendered_row(terminal: &Terminal<TestBackend>, row: u16) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width).map(|x| buffer[(x, row)].symbol()).collect::<String>()
    }

    #[test]
    fn composer_hints_split_when_width_allows() {
        let hints = ComposerHints::normal(96);

        assert_eq!(hints.left, " Enter send  Alt+Enter newline ");
        assert_eq!(hints.right.as_deref(), Some(" Shift+Tab auto  Ctrl+D quit "));
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
    fn empty_composer_uses_short_placeholder() {
        let panel = InputPanel::new();
        let backend = ratatui::backend::TestBackend::new(60, 4);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal.draw(|frame| panel.render(frame, frame.area(), true)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Ask tiny-agent..."));
        assert!(!rendered.contains("edit, inspect, or run"));
    }

    #[test]
    fn composer_render_wraps_long_input_and_keeps_text_close_to_prompt() {
        let mut panel = InputPanel::new();
        set_text(&mut panel, "alpha bravo charlie delta echo");
        let backend = TestBackend::new(24, 6);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| panel.render(frame, frame.area(), true)).unwrap();

        let first_input_row = rendered_row(&terminal, 1);
        let second_input_row = rendered_row(&terminal, 2);
        assert!(
            first_input_row.contains("│› alpha"),
            "first input row should keep a compact two-column prompt: {first_input_row:?}"
        );
        assert!(
            second_input_row.contains("charlie") || second_input_row.contains("delta"),
            "long input should wrap onto the next row: {second_input_row:?}"
        );
    }

    #[test]
    fn composer_cursor_uses_terminal_width_for_wide_characters() {
        let mut panel = InputPanel::new();
        set_text(&mut panel, "你a");
        let backend = TestBackend::new(24, 6);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| panel.render(frame, frame.area(), true)).unwrap();

        terminal.backend_mut().assert_cursor_position(Position::new(6, 1));
    }

    #[test]
    fn desired_height_grows_with_multiline_input() {
        let mut panel = InputPanel::new();
        assert_eq!(panel.desired_height(80, 4, 8), 4);
        assert_eq!(panel.desired_height(80, 3, 8), 3);

        set_text(&mut panel, "line one\nline two\nline three");

        assert_eq!(panel.desired_height(80, 4, 8), 6);
    }

    #[test]
    fn desired_height_grows_with_wrapped_long_input_and_caps() {
        let mut panel = InputPanel::new();
        set_text(&mut panel, "this is a long prompt that should wrap across several terminal rows");

        assert_eq!(panel.desired_height(24, 4, 5), 5);
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
    fn ctrl_shift_up_down_navigates_history() {
        let mut panel = InputPanel::new();
        panel.submit_text("previous".into());

        panel.handle_key(modified_key(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT));
        assert_eq!(text(&panel), "previous");

        panel.handle_key(modified_key(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT));
        assert_eq!(text(&panel), "");
    }

    #[test]
    fn slash_command_popup_accepts_shift_modified_arrows() {
        let mut panel = InputPanel::new();
        panel.handle_key(key(KeyCode::Char('/')));

        panel.handle_key(modified_key(KeyCode::Down, KeyModifiers::SHIFT));
        assert_eq!(panel.popup.selected_command(), Some(&SlashCommand::Model));

        panel.handle_key(modified_key(KeyCode::Up, KeyModifiers::SHIFT));
        assert_eq!(panel.popup.selected_command(), Some(&SlashCommand::Tool));
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

        assert!(!panel.wants_vertical_nav_key(key(KeyCode::Up)));
        assert!(!panel.wants_vertical_nav_key(key(KeyCode::Down)));

        set_text(&mut panel, "line one\nline two");
        assert!(panel.wants_vertical_nav_key(key(KeyCode::Up)));
        assert!(panel.wants_vertical_nav_key(key(KeyCode::Down)));

        panel.clear_text();
        panel.submit_text("previous".into());
        assert!(panel.wants_vertical_nav_key(key(KeyCode::Up)));
        assert!(!panel.wants_vertical_nav_key(key(KeyCode::Down)));

        panel.handle_key(ctrl_key(KeyCode::Up));
        assert_eq!(text(&panel), "previous");
        assert!(panel.wants_vertical_nav_key(key(KeyCode::Down)));
    }

    #[test]
    fn ctrl_a_uses_textarea_default_instead_of_selecting_all() {
        let mut panel = InputPanel::new();
        set_text(&mut panel, "delete all of this");

        panel.handle_key(ctrl_key(KeyCode::Char('a')));
        panel.handle_key(key(KeyCode::Backspace));

        assert_eq!(text(&panel), "delete all of this");
    }

    #[test]
    fn text_returns_multiline_composer_contents() {
        let mut panel = InputPanel::new();
        set_text(&mut panel, "line one\nline two");

        assert_eq!(panel.text(), "line one\nline two");
    }
}
