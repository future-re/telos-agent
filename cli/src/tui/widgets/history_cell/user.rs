use std::any::Any;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::theme::Theme;

use super::HistoryCell;

pub struct UserCell {
    pub content: String,
}

fn user_lines(content: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = vec![Line::from("")];
    lines.extend(content.lines().enumerate().map(|(idx, line)| {
        let marker = if idx == 0 { "▸ " } else { "  " };
        Line::from(vec![
            Span::styled(marker.to_string(), theme.user_style()),
            Span::styled(line.to_string(), theme.user_style()),
        ])
    }));
    lines
}

impl HistoryCell for UserCell {
    fn needed_lines(&self, width: usize) -> u16 {
        let chars_per_line = width.max(20);
        let mut total = 0u16;
        for line in self.content.lines() {
            let line_len = line.len();
            if line_len == 0 {
                total += 1;
            } else {
                total += (line_len as f64 / chars_per_line as f64).ceil() as u16;
            }
        }
        total + 1 // +1 for blank line before user msg (matches current spacing)
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let text = Text::from(user_lines(&self.content, theme));
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        let text = Text::from(user_lines(&self.content, theme));
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: true }).scroll((top_skip, 0)),
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
