use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use tui_textarea::TextArea;

use crate::tui::theme::Theme;

pub struct InputPanel {
    textarea: TextArea<'static>,
}

impl InputPanel {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text(
            "Type a message… (Enter send, Alt+Enter newline, Ctrl+D quit when empty)",
        );
        textarea.set_cursor_line_style(Style::default());
        Self { textarea }
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.lines().join("").trim().is_empty()
    }

    /// Process a key event. Returns `Some(String)` when the user submits.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key {
            KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::NONE, .. } => {
                let text = self.textarea.lines().join("\n");
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                self.textarea.select_all();
                self.textarea.cut();
                Some(trimmed.to_string())
            }
            KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::ALT, .. } => {
                self.textarea.insert_newline();
                None
            }
            _ => {
                self.textarea.input(key);
                None
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, active: bool) {
        let theme = Theme::default();
        let border_style = if active {
            Style::default().fg(theme.border_active)
        } else {
            Style::default().fg(theme.border_inactive)
        };
        let block = Block::default().borders(Borders::TOP).border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(&self.textarea, inner);
    }
}

impl Default for InputPanel {
    fn default() -> Self {
        Self::new()
    }
}
