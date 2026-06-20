use std::time::Duration;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub enum ToolState {
    Pending,
    Running { elapsed: Duration },
    Completed { ok: bool },
}

pub(crate) fn is_shell_tool_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "bash" || lower == "shell"
}

pub(crate) fn tool_title(
    name: &str,
    detail: &str,
    state: &ToolState,
    detail_width: usize,
    colon_separator: bool,
) -> String {
    let detail = truncate_chars(detail.trim(), detail_width);
    if is_shell_tool_name(name) {
        return match state {
            ToolState::Pending | ToolState::Running { .. } => format!("Running {detail}"),
            ToolState::Completed { ok: true } => format!("Ran {detail}"),
            ToolState::Completed { ok: false } => format!("Failed {detail}"),
        };
    }

    if detail.is_empty() {
        name.to_string()
    } else if colon_separator {
        format!("{name}: {detail}")
    } else {
        format!("{name} {detail}")
    }
}

pub(crate) fn push_result_preview(
    lines: &mut Vec<Line<'static>>,
    result_lines: &[String],
    max_lines: usize,
    width: usize,
    theme: &Theme,
    expanded: bool,
    hint_indent: &str,
) {
    for line in result_lines.iter().take(max_lines) {
        lines.push(transcript_line(line, width, theme));
    }
    let hidden = hidden_result_lines(result_lines.len(), max_lines);
    if hidden > 0 {
        let hint = if expanded {
            format!("{hint_indent}… +{hidden} lines")
        } else {
            format!("{hint_indent}… +{hidden} lines (ctrl + t to view transcript)")
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
        )));
    }
}

pub(crate) fn transcript_line(line: &str, width: usize, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("  └ ", Style::default().fg(theme.thinking_fg)),
        Span::styled(
            truncate_chars(line, transcript_width(width)),
            Style::default().fg(theme.thinking_fg),
        ),
    ])
}

pub(crate) fn transcript_width(width: usize) -> usize {
    width.saturating_sub(5).max(16)
}

pub(crate) fn hidden_result_lines(total: usize, shown: usize) -> usize {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ToolState, extract_result_lines, is_shell_tool_name, tool_title, transcript_width,
        truncate_chars,
    };

    #[test]
    fn shell_tool_titles_reflect_state() {
        assert_eq!(
            tool_title("shell", "cargo test --workspace", &ToolState::Pending, 120, false),
            "Running cargo test --workspace"
        );
        assert_eq!(
            tool_title(
                "bash",
                "cargo test --workspace",
                &ToolState::Completed { ok: true },
                120,
                false,
            ),
            "Ran cargo test --workspace"
        );
        assert_eq!(
            tool_title(
                "shell",
                "cargo test --workspace",
                &ToolState::Completed { ok: false },
                120,
                false,
            ),
            "Failed cargo test --workspace"
        );
    }

    #[test]
    fn non_shell_tool_title_uses_separator_when_requested() {
        assert_eq!(
            tool_title("WebSearch", "rust release", &ToolState::Pending, 120, true),
            "WebSearch: rust release"
        );
        assert_eq!(
            tool_title("WebSearch", "rust release", &ToolState::Pending, 120, false),
            "WebSearch rust release"
        );
    }

    #[test]
    fn shared_helpers_preserve_existing_text_behavior() {
        assert!(is_shell_tool_name("Bash"));
        assert!(is_shell_tool_name("shell"));
        assert!(!is_shell_tool_name("WebSearch"));
        assert_eq!(truncate_chars("状态栏测试", 4), "状态栏…");
        assert_eq!(transcript_width(3), 16);
        assert_eq!(transcript_width(120), 115);
    }

    #[test]
    fn extracts_tool_result_lines_from_common_payload_shapes() {
        assert_eq!(
            extract_result_lines(&json!({ "stdout": "one\n\n", "stderr": "two\n" }), false),
            vec!["one".to_string(), "two".to_string()]
        );
        assert_eq!(
            extract_result_lines(&json!({ "text": "hello\nworld" }), false),
            vec!["hello".to_string(), "world".to_string()]
        );
        assert_eq!(
            extract_result_lines(&json!({ "error": { "message": "bad\nworse" } }), true),
            vec!["bad".to_string(), "worse".to_string()]
        );
    }
}
