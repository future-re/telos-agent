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
}

impl HistoryCell for ToolCallCell {
    fn needed_lines(&self, _width: usize) -> u16 {
        // Shell command collapsed: just one line
        if self.is_shell() && !self.expanded {
            return 1;
        }
        let mut lines = 1u16; // tool name line
        lines += self.progress_messages.len() as u16;
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
        // ── Shell command collapsed: show command with [+N] hint ────
        if self.is_shell() && !self.expanded {
            let cmd = if self.detail.chars().count() > 120 {
                format!("{}…", self.detail.chars().take(119).collect::<String>())
            } else {
                self.detail.clone()
            };
            let hint = if self.progress_messages.is_empty() {
                String::new()
            } else {
                format!("  [+{} lines]", self.progress_messages.len())
            };
            let (icon, mut style) = match self.state {
                ToolState::Pending => ("  ◌ ", theme.tool_pending_style()),
                ToolState::Running { .. } => ("  ◌ ", theme.tool_pending_style()),
                ToolState::Completed { ok } => {
                    if ok {
                        ("  ✓ ", theme.tool_ok_style())
                    } else {
                        ("  ✗ ", theme.tool_error_style())
                    }
                }
            };
            if self.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let label = format!("$ {}{}", cmd, hint);
            let line = Line::from(vec![Span::styled(icon, style), Span::styled(label, style)]);
            frame.render_widget(
                Paragraph::new(Text::from(vec![line])).wrap(Wrap { trim: true }),
                area,
            );
            return;
        }

        // ── Full view (non-shell, shell expanded, or pending/running) ──
        let mut spans = Vec::new();

        match self.state {
            ToolState::Pending | ToolState::Running { .. } => {
                let style = if self.selected {
                    theme.tool_pending_style().add_modifier(Modifier::REVERSED)
                } else {
                    theme.tool_pending_style()
                };
                spans.push(Span::styled("  ◌ ", style));
                let label = if self.is_shell() {
                    format!("$ {}", self.detail)
                } else if self.detail.is_empty() {
                    self.name.clone()
                } else {
                    format!("{}: {}", self.name, self.detail)
                };
                spans.push(Span::styled(label, style));
            }
            ToolState::Completed { ok } => {
                let (icon, mut style) = if ok {
                    ("  ✓ ", theme.tool_ok_style())
                } else {
                    ("  ✗ ", theme.tool_error_style())
                };
                if self.selected {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                spans.push(Span::styled(icon, style));
                let label = if self.is_shell() {
                    format!("$ {}", self.detail)
                } else if self.detail.is_empty() {
                    self.name.clone()
                } else {
                    format!("{}: {}", self.name, self.detail)
                };
                spans.push(Span::styled(label, style));
            }
        }

        let mut lines = vec![Line::from(spans)];
        for msg in &self.progress_messages {
            lines.push(Line::from(Span::styled(
                format!("     {msg}"),
                Style::default().fg(theme.thinking_fg),
            )));
        }

        frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }), area);
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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
