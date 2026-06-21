use std::any::Any;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::theme::Theme;
use crate::tui::tool_rendering::{
    extract_result_lines, hidden_result_lines, is_shell_tool_name,
    push_result_preview as push_tool_result_preview, tool_title,
    transcript_line as tool_transcript_line,
};

use super::{HistoryCell, ToolState};

#[derive(Debug, Clone)]
pub struct ToolCallCell {
    pub name: String,
    pub detail: String,
    pub state: ToolState,
    pub tool_call_id: String,
    /// Progress messages accumulated during execution.
    pub progress_messages: Vec<String>,
    /// Final output/error preview lines from the tool result.
    pub result_lines: Vec<String>,
    /// Whether to show expanded output (for shell commands).
    pub expanded: bool,
    /// Whether this cell is selected for keyboard actions.
    pub selected: bool,
}

impl ToolCallCell {
    pub fn new(tool_call_id: String, name: String, detail: String) -> Self {
        let is_shell = is_shell_tool_name(&name);
        Self {
            name,
            detail,
            state: ToolState::Pending,
            tool_call_id,
            progress_messages: Vec::new(),
            result_lines: Vec::new(),
            expanded: !is_shell, // shell commands start collapsed
            selected: false,
        }
    }

    /// Whether this cell represents a shell command execution.
    pub fn is_shell(&self) -> bool {
        is_shell_tool_name(&self.name)
    }

    /// Toggle the expanded/collapsed state.
    pub fn toggle_expand(&mut self) {
        self.expanded = !self.expanded;
    }

    pub fn set_running(&mut self) {
        self.state = ToolState::Running { elapsed: std::time::Duration::ZERO };
    }

    pub fn set_completed(&mut self, ok: bool) {
        self.state = ToolState::Completed { ok };
    }

    pub fn add_progress(&mut self, message: String) {
        self.progress_messages.push(message);
    }

    pub fn add_result_content(&mut self, content: &serde_json::Value, is_error: bool) {
        self.result_lines = extract_result_lines(content, is_error);
    }

    fn title(&self) -> String {
        tool_title(&self.name, &self.detail, &self.state, 120, true)
    }

    fn is_expanded(&self) -> bool {
        self.expanded || !self.is_shell()
    }
}

impl HistoryCell for ToolCallCell {
    fn needed_lines(&self, _width: usize) -> u16 {
        let mut lines = 1u16;
        if self.is_expanded() {
            lines += self.progress_messages.len().min(8) as u16;
            lines += self.result_lines.len().min(12) as u16;
            let hidden = hidden_result_lines(self.result_lines.len(), 12);
            if hidden > 0 {
                lines += 1;
            }
        } else if !self.result_lines.is_empty() {
            lines += self.result_lines.len().min(2) as u16;
            let hidden = hidden_result_lines(self.result_lines.len(), 2);
            if hidden > 0 {
                lines += 1;
            }
        }
        lines
    }

    fn tool_call_id(&self) -> Option<&str> {
        Some(&self.tool_call_id)
    }

    fn is_selectable(&self) -> bool {
        true
    }

    fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let (marker, mut style) = match self.state {
            ToolState::Pending | ToolState::Running { .. } => {
                ("•", Style::default().fg(theme.tool_pending_fg))
            }
            ToolState::Completed { ok: true } if self.is_shell() => {
                ("•", Style::default().fg(theme.assistant_fg).add_modifier(Modifier::BOLD))
            }
            ToolState::Completed { ok: true } => ("✓", theme.tool_ok_style()),
            ToolState::Completed { ok: false } => ("✗", theme.tool_error_style()),
        };
        if self.selected {
            style = style.fg(theme.user_fg).add_modifier(Modifier::BOLD);
        }

        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(self.title(), style),
        ])];

        if self.is_expanded() {
            for msg in self.progress_messages.iter().take(8) {
                lines.push(tool_transcript_line(msg, 185, theme));
            }
            push_tool_result_preview(&mut lines, &self.result_lines, 12, 185, theme, true, "    ");
        } else {
            push_tool_result_preview(&mut lines, &self.result_lines, 2, 185, theme, false, "    ");
        }

        frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }), area);
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        let (marker, mut style) = match self.state {
            ToolState::Pending | ToolState::Running { .. } => {
                ("•", Style::default().fg(theme.tool_pending_fg))
            }
            ToolState::Completed { ok: true } if self.is_shell() => {
                ("•", Style::default().fg(theme.assistant_fg).add_modifier(Modifier::BOLD))
            }
            ToolState::Completed { ok: true } => ("✓", theme.tool_ok_style()),
            ToolState::Completed { ok: false } => ("✗", theme.tool_error_style()),
        };
        if self.selected {
            style = style.fg(theme.user_fg).add_modifier(Modifier::BOLD);
        }

        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(self.title(), style),
        ])];

        if self.is_expanded() {
            for msg in self.progress_messages.iter().take(8) {
                lines.push(tool_transcript_line(msg, 185, theme));
            }
            push_tool_result_preview(&mut lines, &self.result_lines, 12, 185, theme, true, "    ");
        } else {
            push_tool_result_preview(&mut lines, &self.result_lines, 2, 185, theme, false, "    ");
        }

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
