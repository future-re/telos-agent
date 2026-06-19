use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use std::any::Any;
use std::time::Duration;

use crate::tui::theme::Theme;

// ─── Trait ───────────────────────────────────────────────────────────────────

/// A single entry in the chat conversation history.
///
/// Each variant knows how to render itself into a ratatui [`Frame`].
///
/// # Send requirement
/// Cells flow through `mpsc` channels so they must be `Send`.
pub trait HistoryCell: Send {
    /// Number of terminal lines this cell occupies at the given width.
    fn needed_lines(&self, width: usize) -> u16;

    /// Render this cell into `area` of the provided `frame`.
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme);

    /// Render this cell after skipping `top_skip` terminal lines.
    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        if top_skip == 0 {
            self.render(frame, area, theme);
        }
    }

    /// Whether this cell is still accumulating content (streaming).
    fn is_streaming(&self) -> bool {
        false
    }

    /// Append text to a streaming cell. No-op for non-streaming cells.
    fn push_text(&mut self, _text: &str) {}

    /// Mark this cell as no longer receiving streamed content.
    fn finish_streaming(&mut self) {}

    /// Whether this cell can be selected for keyboard actions.
    fn is_selectable(&self) -> bool {
        false
    }

    /// Render this cell as selected.
    fn set_selected(&mut self, _selected: bool) {}

    /// Optional tool_call_id for ToolCallCell lookups.
    fn tool_call_id(&self) -> Option<&str> {
        None
    }

    /// Downcast to &dyn Any for type-specific operations.
    fn as_any(&self) -> &dyn Any;

    /// Downcast to &mut dyn Any for type-specific operations.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ─── UserCell ────────────────────────────────────────────────────────────────

pub struct UserCell {
    pub content: String,
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
        let lines: Vec<Line> = self
            .content
            .lines()
            .map(|line| {
                Line::from(vec![
                    Span::styled("▸ ", theme.user_style()),
                    Span::styled(line.to_string(), theme.user_style()),
                ])
            })
            .collect();

        let text = Text::from(lines);
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        let lines: Vec<Line> = self
            .content
            .lines()
            .map(|line| {
                Line::from(vec![
                    Span::styled("▸ ", theme.user_style()),
                    Span::styled(line.to_string(), theme.user_style()),
                ])
            })
            .collect();

        let text = Text::from(lines);
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

// ─── AgentCell ───────────────────────────────────────────────────────────────

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
        rendered.lines.len() as u16
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
        if is_diff_content(&self.buffer) {
            let diff_text = render_diff(&self.buffer, theme);
            frame.render_widget(Paragraph::new(diff_text).wrap(Wrap { trim: true }), area);
        } else {
            let md_text = crate::tui::markdown::render_markdown(&self.buffer, area.width as usize);
            frame.render_widget(Paragraph::new(md_text).wrap(Wrap { trim: true }), area);
        }
    }

    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        if self.buffer.is_empty() {
            return;
        }
        if is_diff_content(&self.buffer) {
            let diff_text = render_diff(&self.buffer, theme);
            frame.render_widget(
                Paragraph::new(diff_text).wrap(Wrap { trim: true }).scroll((top_skip, 0)),
                area,
            );
        } else {
            let md_text = crate::tui::markdown::render_markdown(&self.buffer, area.width as usize);
            frame.render_widget(
                Paragraph::new(md_text).wrap(Wrap { trim: true }).scroll((top_skip, 0)),
                area,
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

// ─── ThinkingCell ────────────────────────────────────────────────────────────

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

// ─── ToolCallCell

#[derive(Debug, Clone)]
pub enum ToolState {
    Pending,
    Running { elapsed: Duration },
    Completed { ok: bool },
}

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
        let is_shell = matches!(name.to_lowercase().as_str(), "bash" | "shell");
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
        let lower = self.name.to_lowercase();
        lower == "bash" || lower == "shell"
    }

    /// Toggle the expanded/collapsed state.
    pub fn toggle_expand(&mut self) {
        self.expanded = !self.expanded;
    }

    pub fn set_running(&mut self) {
        self.state = ToolState::Running { elapsed: Duration::ZERO };
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
        let detail = truncate_chars(self.detail.trim(), 120);
        if self.is_shell() {
            return match self.state {
                ToolState::Pending | ToolState::Running { .. } => format!("Running {detail}"),
                ToolState::Completed { ok: true } => format!("Ran {detail}"),
                ToolState::Completed { ok: false } => format!("Failed {detail}"),
            };
        }

        if detail.is_empty() { self.name.clone() } else { format!("{}: {}", self.name, detail) }
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
                lines.push(transcript_line(msg, theme));
            }
            push_result_preview(&mut lines, &self.result_lines, 12, theme, true);
        } else {
            push_result_preview(&mut lines, &self.result_lines, 2, theme, false);
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
                lines.push(transcript_line(msg, theme));
            }
            push_result_preview(&mut lines, &self.result_lines, 12, theme, true);
        } else {
            push_result_preview(&mut lines, &self.result_lines, 2, theme, false);
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

fn push_result_preview(
    lines: &mut Vec<Line<'static>>,
    result_lines: &[String],
    max_lines: usize,
    theme: &Theme,
    expanded: bool,
) {
    for line in result_lines.iter().take(max_lines) {
        lines.push(transcript_line(line, theme));
    }
    let hidden = hidden_result_lines(result_lines.len(), max_lines);
    if hidden > 0 {
        let hint = if expanded {
            format!("    … +{hidden} lines")
        } else {
            format!("    … +{hidden} lines (ctrl + t to view transcript)")
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
        )));
    }
}

fn transcript_line(line: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("  └ ", Style::default().fg(theme.thinking_fg)),
        Span::styled(truncate_chars(line, 180), Style::default().fg(theme.thinking_fg)),
    ])
}

fn hidden_result_lines(total: usize, shown: usize) -> usize {
    total.saturating_sub(shown)
}

pub(crate) fn extract_result_lines(content: &serde_json::Value, is_error: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(stdout) = content.get("stdout").and_then(|value| value.as_str()) {
        lines.extend(stdout.lines().map(str::to_string).filter(|line| !line.trim().is_empty()));
    }
    if let Some(stderr) = content.get("stderr").and_then(|value| value.as_str()) {
        lines.extend(stderr.lines().map(str::to_string).filter(|line| !line.trim().is_empty()));
    }
    if lines.is_empty()
        && let Some(text) = content.get("text").and_then(|value| value.as_str())
    {
        lines.extend(text.lines().map(str::to_string).filter(|line| !line.trim().is_empty()));
    }
    if lines.is_empty() && is_error {
        if let Some(message) = content
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|value| value.as_str())
        {
            lines
                .extend(message.lines().map(str::to_string).filter(|line| !line.trim().is_empty()));
        } else {
            lines.push(content.to_string());
        }
    }
    lines
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", text.chars().take(keep).collect::<String>())
}

// ─── TurnSummaryCell ─────────────────────────────────────────────────────────

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
