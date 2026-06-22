//! Conversation history cells — the unit of display in the chat viewport.
//!
//! Every cell implements [`HistoryCell`] with a single `display_lines()` method
//! that produces the logical lines for rendering. Height measurement
//! (`desired_height`) delegates to `Paragraph::line_count()` for accuracy.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use std::any::Any;

use crate::tui::render::Renderable;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

// ─── HistoryCell trait ────────────────────────────────────────────────────────

pub trait HistoryCell: Send + Sync + 'static {
    /// Logical lines for the main chat viewport at the given width.
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Height in terminal rows — measured by wrapping the lines and
    /// asking `Paragraph::line_count()`.
    fn desired_height(&self, width: u16) -> u16 {
        let lines = self.display_lines(width);
        let text = ratatui::text::Text::from(lines);
        let count = Paragraph::new(text).wrap(Wrap { trim: false }).line_count(width);
        count.try_into().unwrap_or(0)
    }

    /// Whether this cell is still receiving streamed content.
    fn is_streaming(&self) -> bool {
        false
    }

    /// Append text delta during streaming.
    fn push_delta(&mut self, _text: &str) {}

    /// Mark streaming as complete.
    fn finish(&mut self) {}

    /// Whether this cell can be selected (for expand/collapse via keyboard).
    fn is_selectable(&self) -> bool {
        false
    }

    /// Set selection state.
    fn set_selected(&mut self, _selected: bool) {}

    /// Optional tool call ID for lookups.
    fn tool_call_id(&self) -> Option<&str> {
        None
    }
}

/// Helper macro for implementing `as_any()` / `as_any_mut()` on concrete cells.
macro_rules! impl_as_any {
    ($ty:ty) => {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    };
}

/// Boxed renderable adapter for `HistoryCell`.
impl Renderable for Box<dyn HistoryCell> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let lines = self.display_lines(area.width);
        let text = ratatui::text::Text::from(lines);
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });

        // Bottom-anchor: compute overflow and scroll to show the bottom.
        let overflow = if area.height == 0 {
            0u16
        } else {
            let line_count: u16 = paragraph.line_count(area.width).try_into().unwrap_or(u16::MAX);
            line_count.saturating_sub(area.height)
        };

        ratatui::widgets::Clear.render(area, buf);
        paragraph.scroll((overflow, 0)).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        HistoryCell::desired_height(self.as_ref(), width)
    }
}

// ─── Cell types ────────────────────────────────────────────────────────────────

// ── User message ──────────────────────────────────────────────────────────────

pub struct UserCell {
    pub content: String,
}

impl HistoryCell for UserCell {
    impl_as_any!(UserCell);
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from("")];
        for (i, line) in self.content.lines().enumerate() {
            let marker = if i == 0 { "▸ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                Span::styled(line.to_string(), Style::default().fg(Color::Cyan)),
            ]));
        }
        lines
    }
}

// ── Agent response ────────────────────────────────────────────────────────────

pub struct AgentCell {
    pub buffer: String,
    pub is_streaming: bool,
}

impl HistoryCell for AgentCell {
    impl_as_any!(AgentCell);
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.buffer.is_empty() {
            return vec![Line::from("")];
        }
        let mut lines = vec![Line::from("")];
        let rendered = crate::tui::markdown::render_markdown(&self.buffer, width as usize);
        lines.extend(rendered.lines);
        lines
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn push_delta(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn finish(&mut self) {
        self.is_streaming = false;
    }
}

// ── Thinking block ────────────────────────────────────────────────────────────

pub struct ThinkingCell {
    pub buffer: String,
    pub is_streaming: bool,
}

impl HistoryCell for ThinkingCell {
    impl_as_any!(ThinkingCell);
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        if self.buffer.is_empty() {
            return vec![];
        }
        let label = format!("  💭 {}", self.buffer.trim());
        label
            .lines()
            .map(|l| {
                Line::from(Span::styled(
                    l.to_string(),
                    Style::default().fg(Color::Rgb(138, 150, 170)),
                ))
            })
            .collect()
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn push_delta(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn finish(&mut self) {
        self.is_streaming = false;
    }
}

// ── Tool call ─────────────────────────────────────────────────────────────────

use crate::tui::tool_rendering::{ToolState, is_shell_tool_name};

pub struct ToolCallCell {
    pub tool_call_id: String,
    pub name: String,
    pub detail: String,
    pub state: ToolState,
    pub progress: Vec<String>,
    pub results: Vec<String>,
    pub expanded: bool,
    pub selected: bool,
}

impl ToolCallCell {
    pub fn new(id: String, name: String, detail: String) -> Self {
        let is_shell = is_shell_tool_name(&name);
        Self {
            tool_call_id: id,
            name,
            detail,
            state: ToolState::Pending,
            progress: Vec::new(),
            results: Vec::new(),
            expanded: !is_shell,
            selected: false,
        }
    }

    pub fn set_running(&mut self) {
        self.state = ToolState::Running { started: std::time::Instant::now() };
    }

    pub fn set_completed(&mut self, ok: bool) {
        self.state = ToolState::Completed { ok };
    }
}

impl HistoryCell for ToolCallCell {
    impl_as_any!(ToolCallCell);
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let w = width as usize;
        let (marker, mut style) = match self.state {
            ToolState::Pending | ToolState::Running { .. } => {
                ("•", Style::default().fg(Color::Rgb(255, 220, 110)))
            }
            ToolState::Completed { ok: true } if is_shell_tool_name(&self.name) => {
                ("•", Style::default().fg(Color::Rgb(110, 220, 145)).add_modifier(Modifier::BOLD))
            }
            ToolState::Completed { ok: true } => {
                ("✓", Style::default().fg(Color::Rgb(110, 220, 145)))
            }
            ToolState::Completed { ok: false } => {
                ("✗", Style::default().fg(Color::Rgb(255, 95, 95)))
            }
        };
        if self.selected {
            style = style.fg(Color::Cyan).add_modifier(Modifier::BOLD);
        }

        let title = crate::tui::tool_rendering::tool_title(
            &self.name,
            &self.detail,
            &self.state,
            120,
            true,
        );

        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(title, style),
        ])];

        if self.expanded {
            for msg in self.progress.iter().take(8) {
                lines.push(crate::tui::tool_rendering::transcript_line(
                    msg,
                    w,
                    &Default::default(),
                ));
            }
            crate::tui::tool_rendering::push_result_preview(
                &mut lines,
                &self.results,
                12,
                w,
                &Default::default(),
                true,
                "    ",
            );
        } else if !self.results.is_empty() {
            crate::tui::tool_rendering::push_result_preview(
                &mut lines,
                &self.results,
                2,
                w,
                &Default::default(),
                false,
                "    ",
            );
        }

        lines
    }

    fn is_selectable(&self) -> bool {
        true
    }

    fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    fn tool_call_id(&self) -> Option<&str> {
        Some(&self.tool_call_id)
    }
}

// ── Separator ─────────────────────────────────────────────────────────────────

pub struct SeparatorCell;

impl HistoryCell for SeparatorCell {
    impl_as_any!(SeparatorCell);
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        vec![
            Line::from(""),
            Line::from(Span::styled("─────", Style::default().fg(Color::Rgb(138, 150, 170)))),
            Line::from(""),
        ]
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

pub struct ErrorCell {
    pub message: String,
}

impl HistoryCell for ErrorCell {
    impl_as_any!(ErrorCell);
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.message
            .lines()
            .map(|l| {
                Line::from(Span::styled(
                    format!("✗ {l}"),
                    Style::default().fg(Color::Rgb(255, 95, 95)),
                ))
            })
            .collect()
    }
}

// ── Turn summary ──────────────────────────────────────────────────────────────

pub struct TurnSummaryCell {
    pub content: String,
}

impl HistoryCell for TurnSummaryCell {
    impl_as_any!(TurnSummaryCell);
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        vec![Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                self.content.clone(),
                Style::default().fg(Color::Rgb(138, 150, 170)).add_modifier(Modifier::DIM),
            ),
        ])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_cell_has_leading_blank_and_marker() {
        let cell = UserCell { content: "hello".into() };
        let lines = cell.display_lines(80);
        assert!(lines[0].to_string().trim().is_empty());
        assert!(lines[1].to_string().contains("▸ hello"));
    }

    #[test]
    fn tool_call_cell_reports_selectable() {
        let cell = ToolCallCell::new("id1".into(), "Bash".into(), "ls".into());
        assert!(cell.is_selectable());
        assert_eq!(cell.tool_call_id(), Some("id1"));
    }

    #[test]
    fn agent_cell_streaming_state() {
        let mut cell = AgentCell { buffer: String::new(), is_streaming: true };
        assert!(cell.is_streaming());
        cell.push_delta("hello");
        cell.finish();
        assert!(!cell.is_streaming());
        assert_eq!(cell.buffer, "hello");
    }

    #[test]
    fn desired_height_delegates_to_line_count() {
        let cell = SeparatorCell;
        let h = cell.desired_height(80);
        assert_eq!(h, 3); // blank + "─────" + blank
    }
}
