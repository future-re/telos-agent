use std::any::Any;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::theme::Theme;

use super::HistoryCell;

pub struct ThinkingCell {
    pub buffer: String,
    pub is_streaming: bool,
}

impl HistoryCell for ThinkingCell {
    fn needed_lines(&self, width: usize) -> u16 {
        if self.buffer.is_empty() {
            return 0;
        }
        let chars_per_line = width.max(20).saturating_sub(3); // "  💭 " prefix
        self.buffer
            .lines()
            .map(|l| {
                if l.is_empty() {
                    1
                } else {
                    (l.len() as f64 / chars_per_line as f64).ceil() as u16
                }
            })
            .sum::<u16>()
            .max(1)
    }

    fn push_text(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn finish_streaming(&mut self) {
        self.is_streaming = false;
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let label = format!("  💭 {}", self.buffer.trim());
        let lines: Vec<Line> = label
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), theme.thinking_style())))
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }), area);
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        let label = format!("  💭 {}", self.buffer.trim());
        let lines: Vec<Line> = label
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), theme.thinking_style())))
            .collect();
        frame.render_widget(
            Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }).scroll((top_skip, 0)),
            area,
        );
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
