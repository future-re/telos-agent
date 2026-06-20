use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tui::theme::Theme;
use crate::tui::tool_rendering::{
    ToolState, is_shell_tool_name, push_result_preview, tool_title, transcript_line,
};

use super::{
    MAX_COMPACT_RESULT_LINES, MAX_EXPANDED_PROGRESS_LINES, expanded_result_line_budget,
    remaining_line_budget,
};

#[derive(Debug, Clone)]
pub(super) struct ToolActivityItem {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) detail: String,
    pub(super) state: ToolState,
    pub(super) progress_messages: Vec<String>,
    pub(super) approval_messages: Vec<String>,
    pub(super) result_lines: Vec<String>,
    pub(super) expanded: bool,
    pub(super) selected: bool,
}

impl ToolActivityItem {
    pub(super) fn new(id: String, name: String, detail: String) -> Self {
        Self {
            id,
            name,
            detail,
            state: ToolState::Pending,
            progress_messages: Vec::new(),
            approval_messages: Vec::new(),
            result_lines: Vec::new(),
            expanded: false,
            selected: false,
        }
    }

    pub(super) fn is_shell(&self) -> bool {
        is_shell_tool_name(&self.name)
    }

    pub(super) fn can_expand(&self) -> bool {
        !self.approval_messages.is_empty()
            || !self.progress_messages.is_empty()
            || !self.result_lines.is_empty()
    }

    pub(super) fn set_running(&mut self) {
        self.state = ToolState::Running { elapsed: std::time::Duration::ZERO };
    }

    pub(super) fn set_completed(&mut self, ok: bool) {
        self.state = ToolState::Completed { ok };
    }

    pub(super) fn summary_name(&self) -> &str {
        self.name.trim()
    }

    fn title(&self, width: usize) -> String {
        tool_title(&self.name, &self.detail, &self.state, width.saturating_sub(14).max(16), false)
    }

    pub(super) fn lines(
        &self,
        width: usize,
        theme: &Theme,
        max_visible_lines: usize,
    ) -> Vec<Line<'static>> {
        let (marker, mut style) = match self.state {
            ToolState::Pending | ToolState::Running { .. } => {
                ("•", Style::default().fg(theme.tool_pending_fg))
            }
            ToolState::Completed { ok: true } if self.is_shell() => {
                ("•", Style::default().fg(theme.assistant_fg).add_modifier(Modifier::BOLD))
            }
            ToolState::Completed { ok: true } => {
                ("•", Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM))
            }
            ToolState::Completed { ok: false } => ("✗", theme.tool_error_style()),
        };
        if self.selected {
            style = style.fg(theme.user_fg).add_modifier(Modifier::BOLD);
        }

        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(self.title(width), style),
        ])];

        if self.can_expand() {
            if !self.expanded {
                if self.is_shell() {
                    push_result_preview(
                        &mut lines,
                        &self.result_lines,
                        MAX_COMPACT_RESULT_LINES,
                        width,
                        theme,
                        self.expanded,
                        "  ",
                    );
                }
                return lines;
            }

            if !self.is_shell() && !self.detail.trim().is_empty() {
                lines.push(transcript_line(
                    &format!("detail: {}", self.detail.trim()),
                    width,
                    theme,
                ));
            }

            for msg in self
                .approval_messages
                .iter()
                .take(remaining_line_budget(lines.len(), max_visible_lines))
            {
                lines.push(transcript_line(msg, width, theme));
            }

            for msg in self.progress_messages.iter().take(MAX_EXPANDED_PROGRESS_LINES) {
                if remaining_line_budget(lines.len(), max_visible_lines) == 0 {
                    break;
                }
                lines.push(transcript_line(msg, width, theme));
            }

            let preview_lines = expanded_result_line_budget(
                lines.len(),
                self.result_lines.len(),
                max_visible_lines,
            );
            if remaining_line_budget(lines.len(), max_visible_lines) > 0 {
                push_result_preview(
                    &mut lines,
                    &self.result_lines,
                    preview_lines,
                    width,
                    theme,
                    self.expanded,
                    "  ",
                );
            }
        }

        lines
    }
}
