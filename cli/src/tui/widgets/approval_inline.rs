use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};

use crate::tui::approval::PendingApproval;
use crate::tui::overlay::truncate_for_popup;
use crate::tui::theme::Theme;

pub const INLINE_APPROVAL_HEIGHT: u16 = 6;
const CONTENT_PADDING: u16 = 2;

pub fn approval_lines(pending: &PendingApproval, width: usize, expanded: bool) -> Vec<String> {
    let request = &pending.request;
    let tool = request.tool_name.trim();
    let content_width = width.saturating_sub(usize::from(CONTENT_PADDING) * 2 + 2).max(24);
    let tool_lower = tool.to_lowercase();
    let mut detail_lines =
        if tool_lower == "bash" || tool_lower == "shell" || tool_lower == "powershell" {
            request.arguments.get("command").and_then(|value| value.as_str()).map_or_else(
                || vec![prefixed_line("Args ", &request.arguments.to_string())],
                |command| command_lines(command, shell_prompt(tool), content_width, expanded),
            )
        } else if tool_lower == "edit" {
            let file =
                request.arguments.get("file_path").and_then(|value| value.as_str()).unwrap_or("?");
            vec![prefixed_line("Edit ", &truncate_for_popup(file, content_width))]
        } else if tool_lower == "write" {
            let file =
                request.arguments.get("file_path").and_then(|value| value.as_str()).unwrap_or("?");
            vec![prefixed_line("Write ", &truncate_for_popup(file, content_width))]
        } else {
            vec![prefixed_line(
                "Args ",
                &truncate_for_popup(&request.arguments.to_string(), content_width),
            )]
        };

    let reason = clean_review_reason(request.reason.trim());
    let reason = if reason.is_empty() {
        "manual review required".to_string()
    } else {
        truncate_for_popup(&reason, content_width)
    };

    let mut lines = vec![format!("Approval · {tool}")];
    lines.append(&mut detail_lines);
    lines.push(format!("Review · {reason}"));
    lines.push("Y · Approve    N · Deny    E · Edit".to_string());
    lines
}

pub fn inline_approval_height(pending: &PendingApproval, width: usize, expanded: bool) -> u16 {
    approval_lines(pending, width, expanded).len().saturating_add(2) as u16
}

fn prefixed_line(prefix: &str, value: &str) -> String {
    format!("{prefix}{value}")
}

fn shell_prompt(tool: &str) -> &'static str {
    if tool.eq_ignore_ascii_case("powershell") { "PS> " } else { "$ " }
}

fn command_lines(command: &str, prefix: &str, width: usize, expanded: bool) -> Vec<String> {
    if !expanded {
        let max_command_width = width.saturating_sub(prefix.len()).max(8);
        return vec![prefixed_line(prefix, &truncate_for_popup(command, max_command_width))];
    }

    wrap_with_prefix(prefix, command, width)
}

fn wrap_with_prefix(prefix: &str, value: &str, width: usize) -> Vec<String> {
    let first_prefix = prefix.to_string();
    let continuation_prefix = " ".repeat(prefix.len());
    let first_width = width.saturating_sub(first_prefix.chars().count()).max(8);
    let continuation_width = width.saturating_sub(continuation_prefix.chars().count()).max(8);
    let mut lines = Vec::new();
    let mut remaining = value.trim();
    let mut first = true;

    while !remaining.is_empty() {
        let line_width = if first { first_width } else { continuation_width };
        let (part, rest) = take_display_chunk(remaining, line_width);
        let prefix = if first { &first_prefix } else { &continuation_prefix };
        lines.push(format!("{prefix}{part}"));
        remaining = rest.trim_start();
        first = false;
    }

    if lines.is_empty() {
        lines.push(first_prefix);
    }
    lines
}

fn take_display_chunk(input: &str, max_chars: usize) -> (&str, &str) {
    if input.chars().count() <= max_chars {
        return (input, "");
    }

    let mut boundary = input.len();
    for (count, (idx, ch)) in input.char_indices().enumerate() {
        if count == max_chars {
            boundary = idx;
            break;
        }
        if ch.is_whitespace() && count > 0 {
            boundary = idx;
        }
    }

    if boundary == input.len() || boundary == 0 {
        boundary = input.char_indices().nth(max_chars).map(|(idx, _)| idx).unwrap_or(input.len());
    }

    input.split_at(boundary)
}

fn clean_review_reason(reason: &str) -> String {
    let prefixes = [
        "shell command needs review:",
        "PowerShell command needs review:",
        "browser action needs review:",
        "tool call needs review:",
    ];
    let trimmed = reason.trim();
    for prefix in prefixes {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    trimmed.to_string()
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    pending: &PendingApproval,
    expanded: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = approval_lines(pending, area.width as usize, expanded);
    let detail_line_count = lines.len().saturating_sub(3);
    let review_idx = detail_line_count + 1;
    let actions_idx = detail_line_count + 2;
    let lines = lines
        .into_iter()
        .enumerate()
        .map(|(idx, text)| {
            let style = if idx == 0 {
                Style::default().fg(theme.assistant_fg).add_modifier(Modifier::BOLD)
            } else if idx <= detail_line_count {
                Style::default().fg(theme.assistant_fg)
            } else if idx == review_idx {
                Style::default().fg(theme.thinking_fg)
            } else if idx == actions_idx {
                Style::default().fg(theme.assistant_fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.assistant_fg)
            };
            Line::from(Span::styled(text, style))
        })
        .collect::<Vec<_>>();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_inactive))
        .padding(Padding::horizontal(CONTENT_PADDING));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use telos_agent::{ApprovalRequest, Message};
    use tokio::sync::oneshot;

    fn pending(tool_name: &str, arguments: serde_json::Value) -> PendingApproval {
        let (tx, _rx) = oneshot::channel();
        PendingApproval {
            request: ApprovalRequest {
                tool_name: tool_name.into(),
                invocation_names: vec![tool_name.into()],
                arguments,
                cwd: PathBuf::from("."),
                messages: Arc::new(vec![Message::user("hi")]),
                reason: "needs review".into(),
            },
            respond: Some(tx),
        }
    }

    #[test]
    fn lines_include_shell_command_and_actions() {
        let lines = approval_lines(&pending("Bash", json!({ "command": "rm target" })), 80, false);
        let text = lines.join("\n");

        assert!(text.contains("Approval · Bash"));
        assert!(text.contains("$ rm target"));
        assert!(text.contains("Bash"));
        assert!(text.contains("rm target"));
        assert!(text.contains("Y · Approve"));
        assert!(text.contains("N · Deny"));
    }

    #[test]
    fn collapsed_shell_command_keeps_small_left_padding() {
        let lines = approval_lines(
            &pending("Bash", json!({ "command": "find . -name '*.md'" })),
            80,
            false,
        );

        assert!(lines[1].starts_with("$ find"));
        assert!(!lines[1].starts_with("Cmd "));
        assert!(!lines[1].starts_with("Command  "));
    }

    #[test]
    fn powershell_command_uses_powershell_prompt() {
        let lines = approval_lines(
            &pending("PowerShell", json!({ "command": "Get-Process pwsh" })),
            80,
            false,
        );

        assert!(lines[1].starts_with("PS> Get-Process"));
    }

    #[test]
    fn expanded_shell_command_wraps_without_truncating() {
        let command =
            "find . -maxdepth 2 -type f -name \"*.md\" -o -name \"*.py\" -o -name \"*.toml\"";
        let lines = approval_lines(&pending("Bash", json!({ "command": command })), 48, true);
        let text = lines.join("\n");

        assert!(lines.len() > 4);
        assert!(!text.contains('…'));
        assert!(text.contains("-name \"*.toml\""));
    }

    #[test]
    fn render_keeps_space_between_border_and_prompt() {
        let pending = pending("Bash", json!({ "command": "find . -maxdepth 1 -type f | sort" }));
        let backend = TestBackend::new(64, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();

        terminal.draw(|frame| render(frame, frame.area(), &theme, &pending, false)).unwrap();
        let buffer = terminal.backend().buffer();
        let width = buffer.area.width as usize;
        let row = &buffer.content[2 * width..3 * width];
        let rendered = row.iter().map(|cell| cell.symbol()).collect::<String>();

        assert!(rendered.starts_with("│  $ find"));
    }

    #[test]
    fn lines_include_reason() {
        let lines = approval_lines(
            &pending("Write", json!({ "file_path": "src/main.rs", "content": "fn main() {}" })),
            80,
            false,
        );
        let text = lines.join("\n");

        assert!(text.contains("needs review"));
        assert!(text.contains("src/main.rs"));
    }

    #[test]
    fn panel_height_fits_content_and_borders() {
        let pending = pending("Bash", json!({ "command": "pwd && ls -la" }));
        let lines = approval_lines(&pending, 80, false);

        assert!(usize::from(inline_approval_height(&pending, 80, false)) >= lines.len() + 2);
    }

    #[test]
    fn shell_review_reason_drops_redundant_prefix() {
        let mut pending = pending("Bash", json!({ "command": "pwd && ls -la" }));
        pending.request.reason =
            "shell command needs review: multiple simple commands (possible command injection)"
                .into();

        let lines = approval_lines(&pending, 100, false);
        let text = lines.join("\n");

        assert!(text.contains("Review · multiple simple commands"));
        assert!(!text.contains("shell command needs review"));
    }
}
