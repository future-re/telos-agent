use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tui_textarea::TextArea;

use crate::tui::theme::Theme;

pub struct InputPanel {
    textarea: TextArea<'static>,
    /// Sent messages — used for up/down history navigation.
    history: Vec<String>,
    /// Current position in history (None = fresh input).
    history_pos: Option<usize>,
    /// Snapshot of the current input before browsing history.
    draft: String,
}

impl InputPanel {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text(
            "Message… (Enter to send, Alt+Enter for newline, Ctrl+D to quit)",
        );
        textarea.set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));
        Self { textarea, history: Vec::new(), history_pos: None, draft: String::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.lines().join("").trim().is_empty()
    }

    /// Process a key event. Returns `Some(String)` when the user submits a prompt.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key {
            // ── Submit ──────────────────────────────────────────────
            KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::NONE, .. } => {
                let text = self.textarea.lines().join("\n");
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    return None;
                }
                // Save to history and reset state.
                self.history.push(trimmed.clone());
                self.history_pos = None;
                self.draft.clear();
                self.textarea.select_all();
                self.textarea.cut();
                Some(trimmed)
            }
            // ── Newline ─────────────────────────────────────────────
            KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::ALT, .. } => {
                self.textarea.insert_newline();
                None
            }
            // ── History: Ctrl+Up ────────────────────────────────────
            KeyEvent { code: KeyCode::Up, modifiers: KeyModifiers::CONTROL, .. }
                if !self.history.is_empty() =>
            {
                self.navigate_history(-1);
                None
            }
            // ── History: Ctrl+Down ──────────────────────────────────
            KeyEvent { code: KeyCode::Down, modifiers: KeyModifiers::CONTROL, .. }
                if !self.history.is_empty() =>
            {
                self.navigate_history(1);
                None
            }
            // ── Default ─────────────────────────────────────────────
            _ => {
                // Any other key resets history browsing.
                if self.history_pos.is_some() {
                    self.history_pos = None;
                }
                self.textarea.input(key);
                None
            }
        }
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
                    // Went past the top — back to draft.
                    self.history_pos = None;
                    let draft = self.draft.clone();
                    self.set_text(&draft);
                    return;
                }
                if np >= len {
                    return; // at newest, stay
                }
                np as usize
            }
            None => {
                // Starting to browse: save current input as draft, go to latest.
                self.draft = self.textarea.lines().join("\n");
                if delta < 0 {
                    (len - 1) as usize
                } else {
                    return; // down at newest → already at draft
                }
            }
        };

        self.history_pos = Some(new_pos);
        let text = self.history[new_pos].clone();
        self.set_text(&text);
    }

    fn set_text(&mut self, text: &str) {
        self.textarea.select_all();
        self.textarea.cut();
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
            Span::styled(" Message ", border_style)
        } else {
            Span::styled(" Streaming… ", Style::default().fg(theme.thinking_fg))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title_top(Line::from(title).left_aligned());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // ── Prompt prefix ──────────────────────────────────────────
        let prompt_w = 2u16; // "> " = 2 columns
        let prompt_area = Rect { x: inner.x, y: inner.y, width: prompt_w, height: 1 };
        let prompt = Paragraph::new(Line::from(Span::styled(
            "> ",
            Style::default().fg(theme.user_fg).add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(prompt, prompt_area);

        // Hint line at the bottom of the block.
        let hint = match self.history_pos {
            Some(i) => format!(" history [{}/{}] ", i + 1, self.history.len()),
            None => String::from(" enter·send  alt+enter·newline  ctrl+↑↓·history "),
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

        // Render the textarea in the remaining space (shifted right for prompt).
        let input_area = Rect {
            x: inner.x + prompt_w,
            y: inner.y,
            width: inner.width.saturating_sub(prompt_w),
            height: inner.height.saturating_sub(1),
        };
        frame.render_widget(&self.textarea, input_area);
    }
}

impl Default for InputPanel {
    fn default() -> Self {
        Self::new()
    }
}
