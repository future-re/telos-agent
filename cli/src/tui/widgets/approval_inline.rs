use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::approval::PendingApproval;
use crate::tui::overlay::truncate_for_popup;
use crate::tui::theme::Theme;

pub const INLINE_APPROVAL_HEIGHT: u16 = 4;

pub fn approval_lines(pending: &PendingApproval, width: usize) -> Vec<String> {
    let request = &pending.request;
    let tool = request.tool_name.trim();
    let detail_width = width.saturating_sub(18).max(24);
    let tool_lower = tool.to_lowercase();
    let detail = if tool_lower == "bash" || tool_lower == "shell" {
        request
            .arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(|command| format!("$ {}", truncate_for_popup(command, detail_width)))
            .unwrap_or_else(|| request.arguments.to_string())
    } else if tool_lower == "edit" {
        let file =
            request.arguments.get("file_path").and_then(|value| value.as_str()).unwrap_or("?");
        format!("edit {}", truncate_for_popup(file, detail_width))
    } else if tool_lower == "write" {
        let file =
            request.arguments.get("file_path").and_then(|value| value.as_str()).unwrap_or("?");
        format!("write {}", truncate_for_popup(file, detail_width))
    } else {
        truncate_for_popup(&request.arguments.to_string(), detail_width)
    };

    let reason = request.reason.trim();
    let reason = if reason.is_empty() {
        "review required".to_string()
    } else {
        truncate_for_popup(reason, detail_width)
    };

    vec![
        format!("Approval required · {tool}"),
        detail,
        reason,
        "y/a approve  n/d deny  e edit".to_string(),
    ]
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
                3 => Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
                _ => Style::default().fg(theme.assistant_fg),
            };
            Line::from(Span::styled(text, style))
        })
        .collect::<Vec<_>>();

    let block =
        Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.tool_pending_fg));

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

        assert!(text.contains("Approval required"));
        assert!(text.contains("Bash"));
        assert!(text.contains("rm target"));
        assert!(text.contains("y/a approve"));
        assert!(text.contains("n/d deny"));
    }

    #[test]
    fn lines_include_reason() {
        let lines = approval_lines(
            &pending(
                "Write",
                json!({ "file_path": "src/main.rs", "content": "fn main() {}" }),
            ),
            80,
        );
        let text = lines.join("\n");

        assert!(text.contains("needs review"));
        assert!(text.contains("src/main.rs"));
    }
}
