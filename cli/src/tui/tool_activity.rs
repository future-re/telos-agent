use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::history_cell::{ToolState, extract_result_lines, truncate_chars};
use crate::tui::theme::Theme;

const MAX_ITEMS: usize = 24;
const MAX_VISIBLE_LINES: usize = 10;
const MAX_EXPANDED_PROGRESS_LINES: usize = 2;
const MAX_COMPACT_RESULT_LINES: usize = 2;

#[derive(Debug, Clone)]
struct ToolActivityItem {
    id: String,
    name: String,
    detail: String,
    state: ToolState,
    progress_messages: Vec<String>,
    approval_messages: Vec<String>,
    result_lines: Vec<String>,
    expanded: bool,
    selected: bool,
}

impl ToolActivityItem {
    fn new(id: String, name: String, detail: String) -> Self {
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

    fn is_shell(&self) -> bool {
        is_shell_name(&self.name)
    }

    fn can_expand(&self) -> bool {
        !self.approval_messages.is_empty()
            || !self.progress_messages.is_empty()
            || !self.result_lines.is_empty()
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

    fn lines(&self, width: usize, theme: &Theme, max_visible_lines: usize) -> Vec<Line<'static>> {
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
                );
            }
        }

        lines
    }
}

#[derive(Debug, Clone)]
pub struct ToolActivityPanel {
    items: Vec<ToolActivityItem>,
    selected_idx: Option<usize>,
    max_visible_lines: usize,
}

impl Default for ToolActivityPanel {
    fn default() -> Self {
        Self { items: Vec::new(), selected_idx: None, max_visible_lines: MAX_VISIBLE_LINES }
    }
}

impl ToolActivityPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_visible_lines(max_visible_lines: usize) -> Self {
        Self { max_visible_lines, ..Self::default() }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.selected_idx = None;
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn height(&self, width: usize) -> u16 {
        self.render_lines(width, &Theme::default()).len().min(self.max_visible_lines) as u16
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
        if let Some(idx) = self.find_idx(id) {
            let was_expandable = self.items[idx].can_expand();
            let item = &mut self.items[idx];
            item.set_running();
            item.progress_messages.push(message);
            self.select_latest_if_became_expandable(idx, was_expandable);
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
        if let Some(idx) = self.find_idx(id) {
            let was_expandable = self.items[idx].can_expand();
            let item = &mut self.items[idx];
            item.result_lines = extract_result_lines(content, is_error);
            self.select_latest_if_became_expandable(idx, was_expandable);
        }
    }

    pub fn approval_requested(&mut self, id: &str, name: String, reason: String) {
        self.annotate(id, name, format!("approval requested: {}", reason.trim()));
    }

    pub fn approval_resolved(&mut self, id: &str, name: String, decision: String) {
        self.annotate(id, name, format!("approval resolved: {}", decision.trim()));
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

    fn find_idx(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }

    fn annotate(&mut self, id: &str, name: String, message: String) {
        if let Some(idx) = self.find_idx(id) {
            let was_expandable = self.items[idx].can_expand();
            self.items[idx].approval_messages.push(message);
            self.select_latest_if_became_expandable(idx, was_expandable);
            return;
        }

        let mut item = ToolActivityItem::new(id.to_string(), name, String::new());
        item.approval_messages.push(message);
        self.items.push(item);
        if self.items.len() > MAX_ITEMS {
            self.items.remove(0);
            self.selected_idx = self.selected_idx.and_then(|idx| idx.checked_sub(1));
        }
        self.select_last_expandable();
    }

    fn render_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let focus_idx = self.focus_idx();

        if let Some(summary) = self.summary_line(focus_idx, width, theme) {
            lines.push(summary);
        }

        if let Some(idx) = focus_idx {
            if let Some(item) = self.items.get(idx) {
                lines.extend(item.lines(width, theme, self.max_visible_lines));
            }
        } else if self.items.len() == 1
            && let Some(item) = self.items.last()
        {
            lines.extend(item.lines(width, theme, self.max_visible_lines));
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

    fn select_latest_if_became_expandable(&mut self, idx: usize, was_expandable: bool) {
        if !was_expandable && idx + 1 == self.items.len() && self.items[idx].can_expand() {
            let keep_expanded = self
                .selected_idx
                .and_then(|selected| self.items.get(selected))
                .is_some_and(|item| item.expanded);
            self.set_selected_idx(Some(idx));
            if keep_expanded {
                self.items[idx].expanded = true;
            }
        }
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

fn expanded_result_line_budget(
    current_lines: usize,
    result_line_count: usize,
    max_visible_lines: usize,
) -> usize {
    let remaining = max_visible_lines.saturating_sub(current_lines);
    if result_line_count > remaining { remaining.saturating_sub(1) } else { remaining }
}

fn remaining_line_budget(current_lines: usize, max_visible_lines: usize) -> usize {
    max_visible_lines.saturating_sub(current_lines)
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

    fn rendered_text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

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
    fn non_shell_activity_can_expand_detail_progress_and_result_preview() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("call-1".into(), "Read".into(), "cli/README.md".into());
        panel.set_progress("call-1", "reading file".into());
        panel.complete("call-1", "Read".into(), true);
        panel.add_result_content(
            "call-1",
            &serde_json::json!({"text": "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n"}),
            false,
        );

        let compact = panel.render_lines(80, &Theme::default());
        assert_eq!(compact.len(), 1);
        assert!(panel.toggle_selected());

        let expanded = panel.render_lines(80, &Theme::default());
        assert_eq!(expanded.len(), MAX_VISIBLE_LINES);
        let rendered = format!("{expanded:?}");
        assert!(rendered.contains("detail: cli/README.md"));
        assert!(rendered.contains("reading file"));
        assert!(rendered.contains("line 1"));
        assert!(rendered.contains("+4 lines"));
    }

    #[test]
    fn compact_max_visible_lines_caps_expanded_activity() {
        let mut panel = ToolActivityPanel::with_max_visible_lines(6);
        panel.push_call("call-1".into(), "Read".into(), "cli/README.md".into());
        panel.set_progress("call-1", "reading file".into());
        panel.complete("call-1", "Read".into(), true);
        panel.add_result_content(
            "call-1",
            &serde_json::json!({"text": "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n"}),
            false,
        );

        assert!(panel.toggle_selected());
        let expanded = panel.render_lines(80, &Theme::default());

        assert_eq!(expanded.len(), 6);
        assert_eq!(panel.height(80), 6);
    }

    #[test]
    fn approval_events_annotate_live_activity() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("call-1".into(), "Bash".into(), "rm important-file".into());

        panel.approval_requested("call-1", "Bash".into(), "requires approval".into());
        panel.approval_resolved("call-1", "Bash".into(), "approved".into());

        assert!(panel.toggle_selected());
        let expanded = panel.render_lines(80, &Theme::default());
        let rendered = format!("{expanded:?}");
        assert!(rendered.contains("approval requested: requires approval"));
        assert!(rendered.contains("approval resolved: approved"));
    }

    #[test]
    fn approval_events_remain_visible_after_prior_progress_when_expanded() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("call-1".into(), "Bash".into(), "rm important-file".into());
        panel.set_progress("call-1", "checking command".into());
        panel.set_progress("call-1", "preparing sandbox".into());

        panel.approval_requested("call-1", "Bash".into(), "requires approval".into());
        panel.approval_resolved("call-1", "Bash".into(), "approved".into());

        assert!(panel.toggle_selected());
        let expanded = panel.render_lines(80, &Theme::default());
        assert!(expanded.len() <= MAX_VISIBLE_LINES);
        let rendered = rendered_text(&expanded);
        assert!(rendered.contains("approval requested: requires approval"));
        assert!(rendered.contains("approval resolved: approved"));
    }

    #[test]
    fn expanded_activity_stays_capped_when_approvals_fill_budget_before_result() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("call-1".into(), "Bash".into(), "deploy".into());
        for idx in 0..MAX_VISIBLE_LINES {
            panel.approval_requested("call-1", "Bash".into(), format!("approval {idx}"));
        }
        panel.add_result_content(
            "call-1",
            &serde_json::json!({"stdout": "line 1\nline 2\nline 3\n", "stderr": ""}),
            false,
        );

        assert!(panel.toggle_selected());
        let expanded = panel.render_lines(80, &Theme::default());

        assert!(expanded.len() <= MAX_VISIBLE_LINES);
    }

    #[test]
    fn latest_non_shell_item_becomes_focused_when_it_gains_result_content() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("old".into(), "Read".into(), "old.md".into());
        panel.set_progress("old", "old progress".into());
        assert!(panel.toggle_selected());

        panel.push_call("new".into(), "Read".into(), "new.md".into());
        panel.add_result_content("new", &serde_json::json!({"text": "new result"}), false);

        let rendered = rendered_text(&panel.render_lines(80, &Theme::default()));
        assert!(rendered.contains("Read new.md"));
        assert!(!rendered.contains("old progress"));
    }

    #[test]
    fn latest_non_shell_item_becomes_focused_when_it_gains_progress() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("old".into(), "Read".into(), "old.md".into());
        panel.set_progress("old", "old progress".into());
        assert!(panel.toggle_selected());

        panel.push_call("new".into(), "Read".into(), "new.md".into());
        panel.set_progress("new", "new progress".into());

        let rendered = rendered_text(&panel.render_lines(80, &Theme::default()));
        assert!(rendered.contains("Read new.md"));
        assert!(rendered.contains("new progress"));
        assert!(!rendered.contains("old progress"));
    }

    #[test]
    fn latest_non_shell_item_becomes_focused_when_it_gains_approval_annotation() {
        let mut panel = ToolActivityPanel::new();
        panel.push_call("old".into(), "Read".into(), "old.md".into());
        panel.set_progress("old", "old progress".into());
        assert!(panel.toggle_selected());

        panel.push_call("new".into(), "Read".into(), "new.md".into());
        panel.approval_requested("new", "Read".into(), "needs approval".into());

        let rendered = rendered_text(&panel.render_lines(80, &Theme::default()));
        assert!(rendered.contains("Read new.md"));
        assert!(rendered.contains("approval requested: needs approval"));
        assert!(!rendered.contains("old progress"));
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
