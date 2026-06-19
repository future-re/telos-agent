use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::tui::approval::PendingApproval;
use crate::tui::theme::Theme;

/// What the app should do after an overlay processes a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    /// Key was consumed, no side effect.
    None,
    /// Pop this overlay from the stack.
    Pop,
    /// Pop and push a different mode.
    Handled,
}

/// A full-screen or floating overlay rendered on top of the base UI.
pub trait Overlay: Send {
    /// Render the overlay (typically over a `Clear`).
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme);

    /// Handle a key event. Returns what the app should do next.
    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction;
}

/// Overlay that shows an approval request popup.
pub struct ApprovalOverlay {
    pub pending: PendingApproval,
}

impl ApprovalOverlay {
    pub fn new(pending: PendingApproval) -> Self {
        Self { pending }
    }

    #[allow(dead_code)]
    fn count_content_lines(&self, width: usize) -> usize {
        let args = &self.pending.request.arguments;
        let tool_name = &self.pending.request.tool_name;
        let inner_w = width.saturating_sub(4).max(40);
        approval_content_lines(tool_name, args, inner_w)
    }
}

impl Overlay for ApprovalOverlay {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let request = &self.pending.request;
        let args = &request.arguments;
        let tool_name = &request.tool_name;

        let block = Block::default()
            .title(" Approval required ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.tool_pending_fg))
            .style(Style::default().bg(Color::Rgb(20, 22, 30)));

        let mut text_lines: Vec<Line> = Vec::new();

        // Tool name line
        text_lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                tool_name.clone(),
                Style::default().fg(theme.tool_pending_fg).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Tool-specific content
        let tool_lower = tool_name.to_lowercase();
        if tool_lower == "bash" || tool_lower == "shell" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                text_lines.push(Line::from(""));
                text_lines.push(Line::from(Span::styled(
                    format!("  $ {}", truncate_for_popup(cmd, 200)),
                    Style::default().fg(Color::Rgb(180, 220, 180)),
                )));
            }
        } else if tool_lower == "edit" {
            let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
            let old = args.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let new = args.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            text_lines.push(Line::from(Span::styled(
                format!("  File: {}", truncate_for_popup(file, 120)),
                Style::default().fg(Color::Gray),
            )));
            text_lines.push(Line::from(""));
            text_lines.push(Line::from(Span::styled(
                format!("  - {}", truncate_for_popup(old, 150)),
                Style::default().fg(Color::Rgb(220, 120, 120)),
            )));
            text_lines.push(Line::from(Span::styled(
                format!("  + {}", truncate_for_popup(new, 150)),
                Style::default().fg(Color::Rgb(120, 220, 120)),
            )));
        } else if tool_lower == "write" {
            let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            text_lines.push(Line::from(Span::styled(
                format!("  File: {}", truncate_for_popup(file, 120)),
                Style::default().fg(Color::Gray),
            )));
            let preview = truncate_for_popup(content, 300);
            if !preview.is_empty() {
                text_lines.push(Line::from(""));
                for pline in preview.lines().take(6) {
                    text_lines.push(Line::from(Span::styled(
                        format!("  | {}", pline),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
        } else {
            let args_str = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
            text_lines.push(Line::from(""));
            for aline in args_str.lines().take(20) {
                text_lines.push(Line::from(Span::styled(
                    format!("  {}", aline),
                    Style::default().fg(Color::Gray),
                )));
            }
        }

        text_lines.push(Line::from(""));
        text_lines.push(Line::from(Span::styled(
            "  [a/y] approve  [d/n] deny  [e] edit-request  ",
            Style::default().fg(Color::White),
        )));

        let text = Text::from(text_lines);
        let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });

        frame.render_widget(Clear, popup_area(area));
        frame.render_widget(paragraph, popup_area(area));
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('y') => {
                if let Some(tx) = self.pending.respond.take() {
                    let _ = tx.send(telos_agent::ApprovalDecision::Allow);
                }
                OverlayAction::Pop
            }
            KeyCode::Char('d') | KeyCode::Char('n') => {
                if let Some(tx) = self.pending.respond.take() {
                    let _ = tx.send(telos_agent::ApprovalDecision::Deny {
                        reason: "denied by user".into(),
                    });
                }
                OverlayAction::Pop
            }
            KeyCode::Char('e') => {
                if let Some(tx) = self.pending.respond.take() {
                    let _ = tx.send(telos_agent::ApprovalDecision::Deny {
                        reason: "edit requested".into(),
                    });
                }
                OverlayAction::Pop
            }
            _ => OverlayAction::None,
        }
    }
}

/// Center a popup in the available area.
fn popup_area(area: Rect) -> Rect {
    let popup_w = area.width.saturating_sub(10).clamp(40, 80);
    let popup_h = area.height.saturating_sub(4).clamp(10, 20);
    Rect {
        x: area.x + (area.width.saturating_sub(popup_w)) / 2,
        y: area.y + (area.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    }
}

/// Count how many lines `text` will occupy when wrapped at `width` columns.
pub fn count_wrapped_lines(text: &str, width: usize) -> usize {
    text.lines()
        .map(|line| {
            let chars = line.chars().count();
            if chars == 0 { 1 } else { (chars + width.saturating_sub(1)) / width }
        })
        .sum::<usize>()
        .max(1)
}

/// Compute the number of lines needed for an approval popup.
pub fn approval_content_lines(tool_name: &str, args: &serde_json::Value, width: usize) -> usize {
    let tool_lower = tool_name.to_lowercase();
    let mut lines = 1usize; // tool name line

    if tool_lower == "bash" || tool_lower == "shell" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            lines += 1; // blank
            lines += count_wrapped_lines(&format!("  $ {}", truncate_for_popup(cmd, 200)), width);
        }
    } else if tool_lower == "edit" {
        let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let old = args.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
        let new = args.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
        lines += count_wrapped_lines(&format!("  File: {}", truncate_for_popup(file, 120)), width);
        lines += 1;
        lines += count_wrapped_lines(&format!("  - {}", truncate_for_popup(old, 150)), width);
        lines += count_wrapped_lines(&format!("  + {}", truncate_for_popup(new, 150)), width);
    } else if tool_lower == "write" {
        let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
        lines += count_wrapped_lines(&format!("  File: {}", truncate_for_popup(file, 120)), width);
        let preview = truncate_for_popup(content, 300);
        if !preview.is_empty() {
            lines += 1;
            for pline in preview.lines().take(6) {
                lines += count_wrapped_lines(&format!("  | {}", pline), width);
            }
        }
    } else {
        let args_str = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
        lines += 1;
        for aline in args_str.lines().take(20) {
            lines += count_wrapped_lines(&format!("  {}", aline), width);
        }
    }
    lines + 1 // hint line
}

/// Truncate a string for display in a popup, adding an ellipsis if truncated.
pub fn truncate_for_popup(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let keep = max_chars.saturating_sub(1);
        format!("{}…", s.chars().take(keep).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_for_popup;

    #[test]
    fn truncate_for_popup_handles_utf8_boundaries() {
        let truncated = truncate_for_popup("中文命令🙂测试", 5);
        assert_eq!(truncated, "中文命令…");
    }

    #[test]
    fn truncate_for_popup_leaves_short_text_unchanged() {
        assert_eq!(truncate_for_popup("hello", 10), "hello");
    }
}
