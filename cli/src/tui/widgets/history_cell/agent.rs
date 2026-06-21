use std::any::Any;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::theme::Theme;

use super::HistoryCell;

pub struct AgentCell {
    pub buffer: String,
    /// When true, this cell is actively receiving text deltas.
    pub is_streaming: bool,
}

// ─── Diff helpers ──────────────────────────────────────────────────────────────

fn is_diff_content(text: &str) -> bool {
    if text.contains("diff --git") {
        return true;
    }
    // Count lines starting with + or - (diff additions/removals, not markdown lists)
    let mut diff_lines = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if (trimmed.starts_with('+') && !trimmed.starts_with("+++"))
            || (trimmed.starts_with('-')
                && !trimmed.starts_with("---")
                && !trimmed.starts_with("- "))
        {
            diff_lines += 1;
        }
        if diff_lines >= 3 {
            return true;
        }
    }
    false
}

fn render_diff(text: &str, theme: &Theme) -> Text<'static> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let span =
            if line.starts_with("diff --git") || line.starts_with("---") || line.starts_with("+++")
            {
                Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )
            } else if line.starts_with("@@") {
                Span::styled(line.to_string(), Style::default().fg(Color::Cyan))
            } else if line.starts_with('+') {
                Span::styled(line.to_string(), Style::default().fg(Color::Rgb(80, 220, 120)))
            } else if line.starts_with('-') {
                Span::styled(line.to_string(), Style::default().fg(Color::Rgb(220, 80, 80)))
            } else {
                Span::styled(line.to_string(), theme.assistant_style())
            };
        lines.push(Line::from(span));
    }
    Text::from(lines)
}

impl HistoryCell for AgentCell {
    fn needed_lines(&self, width: usize) -> u16 {
        if self.buffer.is_empty() {
            return 1;
        }
        // Re-render markdown to measure — simple line count
        let rendered = crate::tui::markdown::render_markdown(&self.buffer, width);
        rendered.lines.len() as u16 + 1
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn push_text(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn finish_streaming(&mut self) {
        self.is_streaming = false;
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.buffer.is_empty() {
            return;
        }
        let content_area = Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        if content_area.height == 0 {
            return;
        }
        if is_diff_content(&self.buffer) {
            let diff_text = render_diff(&self.buffer, theme);
            frame.render_widget(Paragraph::new(diff_text).wrap(Wrap { trim: true }), content_area);
        } else {
            let md_text = crate::tui::markdown::render_markdown(&self.buffer, area.width as usize);
            frame.render_widget(Paragraph::new(md_text).wrap(Wrap { trim: true }), content_area);
        }
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        if self.buffer.is_empty() {
            return;
        }
        let (content_area, adjusted_skip) = if top_skip == 0 {
            let content_area = Rect {
                x: area.x,
                y: area.y.saturating_add(1),
                width: area.width,
                height: area.height.saturating_sub(1),
            };
            (content_area, 0)
        } else {
            (area, top_skip.saturating_sub(1))
        };
        if content_area.height == 0 {
            return;
        }
        if is_diff_content(&self.buffer) {
            let diff_text = render_diff(&self.buffer, theme);
            frame.render_widget(
                Paragraph::new(diff_text).wrap(Wrap { trim: true }).scroll((adjusted_skip, 0)),
                content_area,
            );
        } else {
            let md_text = crate::tui::markdown::render_markdown(&self.buffer, area.width as usize);
            frame.render_widget(
                Paragraph::new(md_text).wrap(Wrap { trim: true }).scroll((adjusted_skip, 0)),
                content_area,
            );
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
