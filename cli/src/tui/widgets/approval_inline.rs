use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::approval::PendingApproval;
use crate::tui::overlay::truncate_for_popup;
use crate::tui::theme::Theme;

pub const INLINE_APPROVAL_HEIGHT: u16 = 6;

pub fn approval_lines(pending: &PendingApproval, width: usize) -> Vec<String> {
    let request = &pending.request;
    let tool = request.tool_name.trim();
    let detail_width = width.saturating_sub(14).max(24);
    let tool_lower = tool.to_lowercase();
    let detail = if tool_lower == "bash" || tool_lower == "shell" {
        request
            .arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(|command| format!("Command  {}", truncate_for_popup(command, detail_width)))
            .unwrap_or_else(|| request.arguments.to_string())
    } else if tool_lower == "edit" {
        let file =
            request.arguments.get("file_path").and_then(|value| value.as_str()).unwrap_or("?");
        format!("Edit     {}", truncate_for_popup(file, detail_width))
    } else if tool_lower == "write" {
        let file =
            request.arguments.get("file_path").and_then(|value| value.as_str()).unwrap_or("?");
        format!("Write    {}", truncate_for_popup(file, detail_width))
    } else {
        format!("Args     {}", truncate_for_popup(&request.arguments.to_string(), detail_width))
    };

    let reason = clean_review_reason(request.reason.trim());
    let reason = if reason.is_empty() {
        "manual review required".to_string()
    } else {
        truncate_for_popup(&reason, detail_width)
    };

    vec![
        format!("Approval · {tool}"),
        detail,
        format!("Review: {reason}"),
        "[Y] Approve   [N] Deny   [E] Edit".to_string(),
    ]
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

pub fn render(frame: &mut Frame, area: Rect, theme: &Theme, pending: &PendingApproval) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = approval_lines(pending, area.width as usize)
        .into_iter()
        .enumerate()
        .map(|(idx, text)| {
            let style = match idx {
                0 => Style::default().fg(theme.tool_pending_fg).add_modifier(Modifier::BOLD),
                1 => Style::default().fg(theme.approval_cmd_fg),
                2 => Style::default().fg(theme.approval_label_fg),
                3 => Style::default().fg(theme.approval_hint_fg).add_modifier(Modifier::BOLD),
                _ => Style::default().fg(theme.assistant_fg),
            };
            Line::from(Span::styled(text, style))
        })
        .collect::<Vec<_>>();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.tool_pending_fg));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let lines = approval_lines(&pending("Bash", json!({ "command": "rm target" })), 80);
        let text = lines.join("\n");

        assert!(text.contains("Approval · Bash"));
        assert!(text.contains("Command"));
        assert!(text.contains("Bash"));
        assert!(text.contains("rm target"));
        assert!(text.contains("[Y] Approve"));
        assert!(text.contains("[N] Deny"));
    }

    #[test]
    fn lines_include_reason() {
        let lines = approval_lines(
            &pending("Write", json!({ "file_path": "src/main.rs", "content": "fn main() {}" })),
            80,
        );
        let text = lines.join("\n");

        assert!(text.contains("needs review"));
        assert!(text.contains("src/main.rs"));
    }

    #[test]
    fn panel_height_fits_content_and_borders() {
        let lines = approval_lines(&pending("Bash", json!({ "command": "pwd && ls -la" })), 80);

        assert!(usize::from(INLINE_APPROVAL_HEIGHT) >= lines.len() + 2);
    }

    #[test]
    fn shell_review_reason_drops_redundant_prefix() {
        let mut pending = pending("Bash", json!({ "command": "pwd && ls -la" }));
        pending.request.reason =
            "shell command needs review: multiple simple commands (possible command injection)"
                .into();

        let lines = approval_lines(&pending, 100);
        let text = lines.join("\n");

        assert!(text.contains("Review: multiple simple commands"));
        assert!(!text.contains("shell command needs review"));
    }
}
