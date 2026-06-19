use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::history_cell::{ToolState, extract_result_lines, truncate_chars};
use crate::tui::theme::Theme;

const MAX_ITEMS: usize = 24;
const MAX_VISIBLE_LINES: u16 = 10;

#[derive(Debug, Clone)]
struct ToolActivityItem {
    id: String,
    name: String,
    detail: String,
    state: ToolState,
    progress_messages: Vec<String>,
    result_lines: Vec<String>,
    expanded: bool,
    selected: bool,
}

impl ToolActivityItem {
    fn new(id: String, name: String, detail: String) -> Self {
        let expanded = !is_shell_name(&name);
        Self {
            id,
            name,
            detail,
            state: ToolState::Pending,
            progress_messages: Vec::new(),
            result_lines: Vec::new(),
            expanded,
            selected: false,
        }
    }

    fn is_shell(&self) -> bool {
        is_shell_name(&self.name)
    }

    fn can_expand(&self) -> bool {
        self.is_shell() && (!self.progress_messages.is_empty() || !self.result_lines.is_empty())
    }

    fn set_running(&mut self) {
        self.state = ToolState::Running { elapsed: std::time::Duration::ZERO };
    }

    fn set_completed(&mut self, ok: bool) {
        self.state = ToolState::Completed { ok };
    }

    fn summary_name(&self) -> &str {
        self.name.trim()
    }

    fn title(&self, width: usize) -> String {
        let detail = truncate_chars(self.detail.trim(), width.saturating_sub(14).max(16));
        if self.is_shell() {
            return match self.state {
                ToolState::Pending | ToolState::Running { .. } => format!("Running {detail}"),
                ToolState::Completed { ok: true } => format!("Ran {detail}"),
                ToolState::Completed { ok: false } => format!("Failed {detail}"),
            };
        }

        if detail.is_empty() { self.name.clone() } else { format!("{} {}", self.name, detail) }
    }

    fn lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
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

        if self.is_shell() {
            let preview_lines = if self.expanded { 8 } else { 2 };
            if self.expanded {
                for msg in self.progress_messages.iter().take(2) {
                    lines.push(transcript_line(msg, width, theme));
                }
            }
            push_result_preview(
                &mut lines,
                &self.result_lines,
                preview_lines,
                width,
                theme,
                self.expanded,
            );
        }

        lines
    }
}

#[derive(Debug, Default, Clone)]
pub struct ToolActivityPanel {
    items: Vec<ToolActivityItem>,
    selected_idx: Option<usize>,
}

impl ToolActivityPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.selected_idx = None;
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn height(&self, width: usize) -> u16 {
        self.render_lines(width, &Theme::default()).len().min(MAX_VISIBLE_LINES as usize) as u16
    }

    pub fn push_call(&mut self, id: String, name: String, detail: String) {
        self.items.push(ToolActivityItem::new(id, name, detail));
        if self.items.len() > MAX_ITEMS {
            self.items.remove(0);
            self.selected_idx = self.selected_idx.and_then(|idx| idx.checked_sub(1));
        }
        self.select_last_expandable();
    }

    pub fn set_progress(&mut self, id: &str, message: String) {
        if let Some(item) = self.find_mut(id) {
            item.set_running();
            item.progress_messages.push(message);
        }
    }

    pub fn complete(&mut self, id: &str, name: String, ok: bool) -> String {
        if let Some(item) = self.find_mut(id) {
            item.set_completed(ok);
            return item.detail.clone();
        }

        let mut item = ToolActivityItem::new(id.to_string(), name, String::new());
        item.set_completed(ok);
        self.items.push(item);
        self.select_last_expandable();
        String::new()
    }

    pub fn add_result_content(&mut self, id: &str, content: &serde_json::Value, is_error: bool) {
        if let Some(item) = self.find_mut(id) {
            item.result_lines = extract_result_lines(content, is_error);
        }
    }

    pub fn select_next(&mut self) {
        self.move_selection(1);
    }

    pub fn select_prev(&mut self) {
        self.move_selection(-1);
    }

    pub fn toggle_selected(&mut self) -> bool {
        if self.selected_idx.is_none() {
            self.select_last_expandable();
        }
        let Some(idx) = self.selected_idx else { return false };
        let Some(item) = self.items.get_mut(idx) else { return false };
        if !item.can_expand() {
            return false;
        }
        item.expanded = !item.expanded;
        true
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.width == 0 || area.height == 0 || self.items.is_empty() {
            return;
        }
        let mut lines = self.render_lines(area.width as usize, theme);
        if lines.len() > area.height as usize {
            lines = lines.split_off(lines.len() - area.height as usize);
        }
        frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }), area);
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut ToolActivityItem> {
        self.items.iter_mut().find(|item| item.id == id)
    }

    fn render_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let focus_idx = self.focus_idx();

        if let Some(summary) = self.summary_line(focus_idx, width, theme) {
            lines.push(summary);
        }

        if let Some(idx) = focus_idx {
            if let Some(item) = self.items.get(idx) {
                lines.extend(item.lines(width, theme));
            }
        } else if self.items.len() == 1
            && let Some(item) = self.items.last()
        {
            lines.extend(item.lines(width, theme));
        }
        lines
    }

    fn focus_idx(&self) -> Option<usize> {
        self.selected_idx.or_else(|| self.items.iter().rposition(ToolActivityItem::can_expand))
    }

    fn summary_line(
        &self,
        focus_idx: Option<usize>,
        width: usize,
        theme: &Theme,
    ) -> Option<Line<'static>> {
        if focus_idx.is_none() && self.items.len() <= 1 {
            return None;
        }

        let summarized = self
            .items
            .iter()
            .enumerate()
            .filter(|(idx, _)| Some(*idx) != focus_idx)
            .map(|(_, item)| item)
            .collect::<Vec<_>>();
        if summarized.is_empty() {
            return None;
        }

        let mut tool_counts: Vec<(&str, usize)> = Vec::new();
        let mut command_count = 0usize;
        let mut running_count = 0usize;
        let mut failed_count = 0usize;

        for item in summarized {
            match item.state {
                ToolState::Pending | ToolState::Running { .. } => running_count += 1,
                ToolState::Completed { ok: false } => failed_count += 1,
                ToolState::Completed { ok: true } => {}
            }

            if item.is_shell() {
                command_count += 1;
                continue;
            }

            let name = item.summary_name();
            if name.is_empty() {
                continue;
            }
            if let Some((_, count)) =
                tool_counts.iter_mut().find(|(candidate, _)| *candidate == name)
            {
                *count += 1;
            } else {
                tool_counts.push((name, 1));
            }
        }

        let mut parts = Vec::new();
        if running_count > 0 {
            parts.push(format!("{running_count} running"));
        }
        if failed_count > 0 {
            parts.push(format!("{failed_count} failed"));
        }
        if command_count > 0 {
            parts.push(format!("{command_count} cmd"));
        }

        let tool_label = tool_counts
            .into_iter()
            .take(5)
            .map(
                |(name, count)| {
                    if count == 1 { name.to_string() } else { format!("{name} x{count}") }
                },
            )
            .collect::<Vec<_>>()
            .join(", ");
        if !tool_label.is_empty() {
            parts.push(tool_label);
        }

        if parts.is_empty() {
            return None;
        }

        let summary = truncate_chars(
            &format!("Activity {}", parts.join(" · ")),
            width.saturating_sub(2).max(16),
        );
        Some(Line::from(vec![
            Span::styled("• ", Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM)),
            Span::styled(
                summary,
                Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
            ),
        ]))
    }

    fn move_selection(&mut self, delta: isize) {
        let selectable: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| item.can_expand().then_some(idx))
            .collect();
        if selectable.is_empty() {
            self.selected_idx = None;
            return;
        }

        let current_pos = self
            .selected_idx
            .and_then(|idx| selectable.iter().position(|candidate| *candidate == idx));
        let next_pos = match current_pos {
            Some(pos) => {
                let len = selectable.len() as isize;
                (pos as isize + delta).rem_euclid(len) as usize
            }
            None if delta < 0 => selectable.len() - 1,
            None => 0,
        };
        self.set_selected_idx(Some(selectable[next_pos]));
    }

    fn select_last_expandable(&mut self) {
        let idx = self.items.iter().rposition(ToolActivityItem::can_expand);
        self.set_selected_idx(idx);
    }

    fn set_selected_idx(&mut self, idx: Option<usize>) {
        if let Some(old) = self.selected_idx
            && let Some(item) = self.items.get_mut(old)
        {
            item.selected = false;
        }
        self.selected_idx = idx;
        if let Some(new) = self.selected_idx
            && let Some(item) = self.items.get_mut(new)
        {
            item.selected = true;
        }
    }
}

fn is_shell_name(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "bash" | "shell")
}

fn push_result_preview(
    lines: &mut Vec<Line<'static>>,
    result_lines: &[String],
    max_lines: usize,
    width: usize,
    theme: &Theme,
    expanded: bool,
) {
    for line in result_lines.iter().take(max_lines) {
        lines.push(transcript_line(line, width, theme));
    }
    let hidden = result_lines.len().saturating_sub(max_lines);
    if hidden > 0 {
        let hint = if expanded {
            format!("  … +{hidden} lines")
        } else {
            format!("  … +{hidden} lines (ctrl + t to view transcript)")
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
        )));
    }
}

fn transcript_line(line: &str, width: usize, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("  └ ", Style::default().fg(theme.thinking_fg)),
        Span::styled(
            truncate_chars(line, width.saturating_sub(5).max(16)),
            Style::default().fg(theme.thinking_fg),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_activity_keeps_folded_transcript_preview() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("call-1".into(), "Bash".into(), "cargo test".into());
        panel.complete("call-1", "Bash".into(), true);
        panel.add_result_content(
            "call-1",
            &serde_json::json!({"stdout": "ok\nnext\nthird\n", "stderr": ""}),
            false,
        );

        let lines = panel.render_lines(80, &Theme::default());
        assert_eq!(lines.len(), 4);
        assert!(format!("{:?}", lines[0]).contains("Ran cargo test"));
        assert!(format!("{:?}", lines[3]).contains("ctrl + t"));
    }

    #[test]
    fn non_shell_activity_stays_single_line() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("call-1".into(), "Read".into(), "cli/README.md".into());
        panel.complete("call-1", "Read".into(), true);

        assert_eq!(panel.render_lines(80, &Theme::default()).len(), 1);
    }

    #[test]
    fn small_tools_are_summarized_instead_of_stacked() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("read-1".into(), "Read".into(), "a.md".into());
        panel.complete("read-1", "Read".into(), true);
        panel.push_call("read-2".into(), "Read".into(), "b.md".into());
        panel.complete("read-2", "Read".into(), true);
        panel.push_call("glob-1".into(), "Glob".into(), "*.md".into());
        panel.complete("glob-1", "Glob".into(), true);

        let lines = panel.render_lines(80, &Theme::default());
        assert_eq!(lines.len(), 1);
        let rendered = format!("{:?}", lines[0]);
        assert!(rendered.contains("Activity"));
        assert!(rendered.contains("Read x2"));
        assert!(rendered.contains("Glob"));
    }
}
