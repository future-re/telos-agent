//! Input composer — wraps `tui_textarea` with a border and prompt.
//!
//! Implements [`Renderable`] so it can be placed in a `FlexRenderable` layout.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use tui_textarea::TextArea;

use crate::tui::render::Renderable;

pub struct Composer {
    textarea: TextArea<'static>,
    pub history: Vec<String>,
    _history_pos: Option<usize>,
    pub active: bool,
    pub turn_active: bool,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Ask anything…");
        Self { textarea, history: Vec::new(), _history_pos: None, active: true, turn_active: false }
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    pub fn clear(&mut self) {
        self.textarea = TextArea::default();
    }

    pub fn set_text(&mut self, text: &str) {
        self.textarea = TextArea::from([text.to_string()]);
    }

    pub fn insert_text(&mut self, text: &str) {
        self.textarea.insert_str(text);
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.textarea.input(key)
    }

    pub fn record_history(&mut self, text: String) {
        if !text.is_empty() {
            self.history.push(text);
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn replace_history(&mut self, items: Vec<String>) {
        self.history = items;
    }
}

impl Renderable for Composer {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.turn_active {
            Color::Rgb(100, 200, 245)
        } else if self.active {
            Color::Rgb(110, 220, 145)
        } else {
            Color::Rgb(80, 80, 80)
        };

        let title = if self.turn_active {
            " Running… "
        } else if self.active {
            " Compose "
        } else {
            " Streaming… "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Line::from(Span::styled(
                title,
                Style::default().fg(border_color).add_modifier(Modifier::BOLD),
            )));

        let inner = block.inner(area);
        block.render(area, buf);

        // Render prompt
        let prompt_area = Rect { x: inner.x, y: inner.y, width: 2, height: 1 };
        Paragraph::new(Line::from(Span::styled(
            "› ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )))
        .render(prompt_area, buf);

        // Render textarea content
        let text_area = Rect {
            x: inner.x + 2,
            y: inner.y,
            width: inner.width.saturating_sub(2),
            height: inner.height,
        };
        self.textarea.render(text_area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let lines = self.textarea.lines().len().max(1);
        (lines + 2) as u16 // +2 for border
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let inner = Block::default().borders(Borders::ALL).inner(area);
        let (col, row) = self.textarea.cursor();
        Some((inner.x + col as u16 + 2, inner.y + row as u16))
    }
}
