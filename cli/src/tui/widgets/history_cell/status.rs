use std::any::Any;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::theme::Theme;

use super::HistoryCell;

pub struct TurnSummaryCell {
    pub content: String,
}

impl HistoryCell for TurnSummaryCell {
    fn needed_lines(&self, width: usize) -> u16 {
        let chars_per_line = width.max(20).saturating_sub(2);
        ((self.content.chars().count() as f64 / chars_per_line as f64).ceil() as u16).max(1)
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                self.content.clone(),
                Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
            ),
        ]);
        frame.render_widget(Paragraph::new(Text::from(vec![line])).wrap(Wrap { trim: true }), area);
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                self.content.clone(),
                Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(Text::from(vec![line])).wrap(Wrap { trim: true }).scroll((top_skip, 0)),
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

// ─── SeparatorCell ───────────────────────────────────────────────────────────

pub struct SeparatorCell;

impl HistoryCell for SeparatorCell {
    fn needed_lines(&self, _width: usize) -> u16 {
        3 // blank + separator + blank
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled("─────", Style::default().fg(theme.thinking_fg))),
            Line::from(""),
        ];
        frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }), area);
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled("─────", Style::default().fg(theme.thinking_fg))),
            Line::from(""),
        ];
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

// ─── ErrorCell ───────────────────────────────────────────────────────────────

pub struct ErrorCell {
    pub message: String,
}

impl HistoryCell for ErrorCell {
    fn needed_lines(&self, width: usize) -> u16 {
        let chars_per_line = width.max(20).saturating_sub(2); // "✗ " prefix
        self.message
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

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let lines: Vec<Line> = self
            .message
            .lines()
            .map(|l| Line::from(Span::styled(format!("✗ {l}"), theme.tool_error_style())))
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }), area);
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        let lines: Vec<Line> = self
            .message
            .lines()
            .map(|l| Line::from(Span::styled(format!("✗ {l}"), theme.tool_error_style())))
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
