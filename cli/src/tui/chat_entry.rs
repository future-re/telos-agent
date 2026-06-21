//! Single `ChatEntry` enum that replaces the `HistoryCell` trait hierarchy.
//!
//! Every entry knows how to render itself to [`Line`]s via [`ChatEntry::to_lines`].
//! Because measurement and rendering share the same code path, layout
//! computations are always consistent with what appears on screen.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tui::theme::Theme;
use crate::tui::tool_rendering::{
    self, ToolState, is_shell_tool_name, push_result_preview, tool_title,
};

// ─── ChatEntry ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ChatEntry {
    /// A message from the user.
    User { content: String },
    /// An assistant response (may be streaming).
    Agent { buffer: String, is_streaming: bool },
    /// A thinking / chain-of-thought block (may be streaming).
    Thinking { buffer: String, is_streaming: bool },
    /// A tool invocation.
    ToolCall {
        id: String,
        name: String,
        detail: String,
        state: ToolState,
        progress: Vec<String>,
        results: Vec<String>,
        expanded: bool,
        selected: bool,
    },
    /// A visual separator between turns.
    Separator,
    /// An error message.
    Error { message: String },
    /// A turn summary line.
    TurnSummary { content: String },
}

// ─── Construction helpers ──────────────────────────────────────────────────────

impl ChatEntry {
    pub fn user(content: String) -> Self {
        ChatEntry::User { content }
    }

    pub fn agent(buffer: String, is_streaming: bool) -> Self {
        ChatEntry::Agent { buffer, is_streaming }
    }

    pub fn thinking(buffer: String, is_streaming: bool) -> Self {
        ChatEntry::Thinking { buffer, is_streaming }
    }

    pub fn tool_call(id: String, name: String, detail: String) -> Self {
        let is_shell = is_shell_tool_name(&name);
        ChatEntry::ToolCall {
            id,
            name,
            detail,
            state: ToolState::Pending,
            progress: Vec::new(),
            results: Vec::new(),
            expanded: !is_shell, // shell commands start collapsed
            selected: false,
        }
    }

    pub fn error(message: String) -> Self {
        ChatEntry::Error { message }
    }

    pub fn separator() -> Self {
        ChatEntry::Separator
    }

    pub fn turn_summary(content: String) -> Self {
        ChatEntry::TurnSummary { content }
    }
}

// ─── Streaming helpers ─────────────────────────────────────────────────────────

impl ChatEntry {
    pub fn is_streaming(&self) -> bool {
        matches!(
            self,
            ChatEntry::Agent { is_streaming: true, .. }
                | ChatEntry::Thinking { is_streaming: true, .. }
        )
    }

    pub fn push_text(&mut self, text: &str) {
        match self {
            ChatEntry::Agent { buffer, .. } | ChatEntry::Thinking { buffer, .. } => {
                buffer.push_str(text);
            }
            _ => {}
        }
    }

    pub fn finish_streaming(&mut self) {
        match self {
            ChatEntry::Agent { is_streaming, .. } => *is_streaming = false,
            ChatEntry::Thinking { is_streaming, .. } => *is_streaming = false,
            _ => {}
        }
    }

    pub fn is_selectable(&self) -> bool {
        matches!(self, ChatEntry::ToolCall { .. })
    }

    pub fn set_selected(&mut self, selected: bool) {
        if let ChatEntry::ToolCall { selected: sel, .. } = self {
            *sel = selected;
        }
    }

    pub fn tool_call_id(&self) -> Option<&str> {
        match self {
            ChatEntry::ToolCall { id, .. } => Some(id),
            _ => None,
        }
    }

    // ── Tool-call-specific mutations ────────────────────────────────────

    pub fn set_running(&mut self) {
        if let ChatEntry::ToolCall { state, .. } = self {
            *state = ToolState::Running { started: std::time::Instant::now() };
        }
    }

    pub fn set_completed(&mut self, ok: bool) {
        if let ChatEntry::ToolCall { state, .. } = self {
            *state = ToolState::Completed { ok };
        }
    }

    pub fn add_progress(&mut self, message: String) {
        if let ChatEntry::ToolCall { progress, .. } = self {
            progress.push(message);
        }
    }

    pub fn add_result_content(&mut self, content: &serde_json::Value, is_error: bool) {
        if let ChatEntry::ToolCall { results, .. } = self {
            *results = tool_rendering::extract_result_lines(content, is_error);
        }
    }

    pub fn toggle_expand(&mut self) {
        if let ChatEntry::ToolCall { expanded, .. } = self {
            *expanded = !*expanded;
        }
    }

    pub fn is_expanded(&self) -> bool {
        match self {
            ChatEntry::ToolCall { expanded, name, .. } => *expanded || !is_shell_tool_name(name),
            _ => false,
        }
    }

    pub fn is_shell(&self) -> bool {
        match self {
            ChatEntry::ToolCall { name, .. } => is_shell_tool_name(name),
            _ => false,
        }
    }
}

// ─── Rendering ─────────────────────────────────────────────────────────────────

impl ChatEntry {
    /// Render this entry into terminal lines at the given width.
    ///
    /// This single method replaces `needed_lines`, `render`, and
    /// `render_scrolled` — measurement and rendering are identical.
    pub fn to_lines(&self, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            ChatEntry::User { content } => user_lines(content, theme),
            ChatEntry::Agent { buffer, .. } => agent_lines(buffer, width, theme),
            ChatEntry::Thinking { buffer, .. } => thinking_lines(buffer, theme),
            ChatEntry::ToolCall {
                name,
                detail,
                state,
                progress,
                results,
                expanded,
                selected,
                ..
            } => tool_call_lines(
                name, detail, state, progress, results, *expanded, *selected, width, theme,
            ),
            ChatEntry::Separator => vec![
                Line::from(""),
                Line::from(Span::styled("─────", Style::default().fg(theme.thinking_fg))),
                Line::from(""),
            ],
            ChatEntry::Error { message } => error_lines(message, theme),
            ChatEntry::TurnSummary { content } => turn_summary_lines(content, theme),
        }
    }
}

// ─── Per-variant line builders ─────────────────────────────────────────────────

fn user_lines(content: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = vec![Line::from("")]; // blank before user message
    lines.extend(content.lines().enumerate().map(|(idx, line)| {
        let marker = if idx == 0 { "▸ " } else { "  " };
        Line::from(vec![
            Span::styled(marker.to_string(), theme.user_style()),
            Span::styled(line.to_string(), theme.user_style()),
        ])
    }));
    lines
}

fn agent_lines(buffer: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    if buffer.is_empty() {
        return vec![Line::from("")];
    }
    let mut lines = vec![Line::from("")]; // leading blank for spacing
    if is_diff_content(buffer) {
        for line in buffer.lines() {
            lines.push(diff_line(line, theme));
        }
    } else {
        let rendered = crate::tui::markdown::render_markdown(buffer, width);
        lines.extend(rendered.lines.iter().cloned());
    }
    lines
}

fn thinking_lines(buffer: &str, theme: &Theme) -> Vec<Line<'static>> {
    if buffer.is_empty() {
        return vec![];
    }
    let label = format!("  💭 {}", buffer.trim());
    label.lines().map(|l| Line::from(Span::styled(l.to_string(), theme.thinking_style()))).collect()
}

fn tool_call_lines(
    name: &str,
    detail: &str,
    state: &ToolState,
    progress: &[String],
    results: &[String],
    expanded: bool,
    selected: bool,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let (marker, mut style) = match state {
        ToolState::Pending | ToolState::Running { .. } => {
            ("•", Style::default().fg(theme.tool_pending_fg))
        }
        ToolState::Completed { ok: true } if is_shell_tool_name(name) => {
            ("•", Style::default().fg(theme.assistant_fg).add_modifier(Modifier::BOLD))
        }
        ToolState::Completed { ok: true } => ("✓", theme.tool_ok_style()),
        ToolState::Completed { ok: false } => ("✗", theme.tool_error_style()),
    };
    if selected {
        style = style.fg(theme.user_fg).add_modifier(Modifier::BOLD);
    }

    let title = tool_title(name, detail, state, 120, true);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{marker} "), style),
        Span::styled(title, style),
    ])];

    if expanded {
        for msg in progress.iter().take(8) {
            lines.push(tool_rendering::transcript_line(msg, width, theme));
        }
        push_result_preview(&mut lines, results, 12, width, theme, true, "    ");
    } else if !results.is_empty() {
        push_result_preview(&mut lines, results, 2, width, theme, false, "    ");
    }

    lines
}

fn error_lines(message: &str, theme: &Theme) -> Vec<Line<'static>> {
    message
        .lines()
        .map(|l| Line::from(Span::styled(format!("✗ {l}"), theme.tool_error_style())))
        .collect()
}

fn turn_summary_lines(content: &str, theme: &Theme) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            content.to_string(),
            Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
        ),
    ])]
}

// ─── Diff helpers ──────────────────────────────────────────────────────────────

fn is_diff_content(text: &str) -> bool {
    if text.contains("diff --git") {
        return true;
    }
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

fn diff_line(line: &str, theme: &Theme) -> Line<'static> {
    let style =
        if line.starts_with("diff --git") || line.starts_with("---") || line.starts_with("+++") {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if line.starts_with("@@") {
            Style::default().fg(Color::Cyan)
        } else if line.starts_with('+') {
            Style::default().fg(Color::Rgb(80, 220, 120))
        } else if line.starts_with('-') {
            Style::default().fg(Color::Rgb(220, 80, 80))
        } else {
            theme.assistant_style()
        };
    Line::from(Span::styled(line.to_string(), style))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_entry_has_leading_blank_and_prompt_marker() {
        let theme = Theme::default();
        let lines = ChatEntry::user("hello".into()).to_lines(80, &theme);
        // First line is blank spacer
        assert!(lines[0].to_string().trim().is_empty());
        // Second line has the content with ▸ prefix
        let content = lines[1].to_string();
        assert!(content.contains("▸"));
        assert!(content.contains("hello"));
    }

    #[test]
    fn agent_entry_renders_markdown() {
        let theme = Theme::default();
        let lines = ChatEntry::agent("**bold** text".into(), false).to_lines(80, &theme);
        let text = lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        // First line is blank spacer
        assert!(text.contains("bold"));
    }

    #[test]
    fn thinking_entry_empty_buffer_returns_empty() {
        let theme = Theme::default();
        let lines = ChatEntry::thinking(String::new(), false).to_lines(80, &theme);
        assert!(lines.is_empty());
    }

    #[test]
    fn separator_has_three_lines() {
        let theme = Theme::default();
        let lines = ChatEntry::separator().to_lines(80, &theme);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn tool_call_is_selectable() {
        let tool = ChatEntry::tool_call("id1".into(), "Bash".into(), "ls".into());
        assert!(tool.is_selectable());
    }

    #[test]
    fn user_entry_not_selectable() {
        let user = ChatEntry::user("hi".into());
        assert!(!user.is_selectable());
    }

    #[test]
    fn streaming_entries_report_is_streaming() {
        assert!(ChatEntry::agent("x".into(), true).is_streaming());
        assert!(ChatEntry::thinking("x".into(), true).is_streaming());
        assert!(!ChatEntry::agent("x".into(), false).is_streaming());
        assert!(!ChatEntry::user("x".into()).is_streaming());
    }

    #[test]
    fn tool_call_lines_respect_width() {
        let theme = Theme::default();
        let mut tool = ChatEntry::tool_call("id1".into(), "Bash".into(), "echo hello world".into());
        tool.add_result_content(&serde_json::json!({"stdout": "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\nline11\nline12\nline13"}), false);

        let narrow = tool.to_lines(40, &theme);
        let wide = tool.to_lines(120, &theme);
        // Wider terminal should produce the same or fewer lines
        // (because content wraps less)
        assert!(wide.len() <= narrow.len());
    }
}
