use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::app::UiMessage;
use crate::tui::theme::Theme;

pub struct ChatPanel {
    /// Number of lines scrolled back from the bottom.
    pub scroll_offset: usize,
}

impl ChatPanel {
    pub fn new() -> Self {
        Self { scroll_offset: 0 }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    fn render_messages(&self, messages: &[UiMessage], width: usize) -> Text<'static> {
        let theme = Theme::default();
        let mut lines: Vec<Line> = Vec::new();
        let mut md_buf = String::new();
        let mut think_buf = String::new();

        /// Flush accumulated markdown buffer into lines.
        fn flush_md(lines: &mut Vec<Line>, buf: &mut String, width: usize) {
            if !buf.is_empty() {
                let md_text = crate::tui::markdown::render_markdown(buf, width);
                for line in md_text.lines {
                    lines.push(line);
                }
                buf.clear();
            }
        }

        /// Flush accumulated thinking buffer — show full content, dimmed.
        fn flush_think(lines: &mut Vec<Line>, buf: &mut String, theme: &Theme) {
            if !buf.is_empty() {
                let label = format!("  💭 {}", buf.trim());
                for line in label.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), theme.thinking_style())));
                }
                buf.clear();
            }
        }

        for msg in messages {
            match msg {
                UiMessage::User(content) => {
                    flush_md(&mut lines, &mut md_buf, width);
                    flush_think(&mut lines, &mut think_buf, &theme);
                    if !lines.is_empty() {
                        lines.push(Line::from("")); // blank line before user
                    }
                    for line in content.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("▸ ", theme.user_style()),
                            Span::styled(line.to_string(), theme.user_style()),
                        ]));
                    }
                }
                UiMessage::AssistantDelta(text) => {
                    flush_think(&mut lines, &mut think_buf, &theme);
                    md_buf.push_str(text);
                }
                UiMessage::ThinkingDelta(text) => {
                    flush_md(&mut lines, &mut md_buf, width);
                    think_buf.push_str(text);
                }
                UiMessage::ToolCall { name, detail, .. } => {
                    flush_md(&mut lines, &mut md_buf, width);
                    flush_think(&mut lines, &mut think_buf, &theme);
                    let label = if detail.is_empty() {
                        name.clone()
                    } else {
                        format!("{}: {}", name, detail)
                    };
                    lines.push(Line::from(vec![
                        Span::styled("  ◌ ", theme.tool_pending_style()),
                        Span::styled(label, theme.tool_pending_style()),
                    ]));
                }
                UiMessage::ToolProgress { message, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("     ", Style::default()),
                        Span::styled(message.clone(), Style::default().fg(theme.thinking_fg)),
                    ]));
                }
                UiMessage::ToolCompleted { name, detail, is_error, .. } => {
                    flush_md(&mut lines, &mut md_buf, width);
                    flush_think(&mut lines, &mut think_buf, &theme);
                    let (icon, style) = if *is_error {
                        ("  ✗ ", theme.tool_error_style())
                    } else {
                        ("  ✓ ", theme.tool_ok_style())
                    };
                    let label = if detail.is_empty() {
                        name.clone()
                    } else {
                        format!("{}: {}", name, detail)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(icon, style),
                        Span::styled(label, style),
                    ]));
                }
                UiMessage::Error(message) => {
                    flush_md(&mut lines, &mut md_buf, width);
                    flush_think(&mut lines, &mut think_buf, &theme);
                    for line in message.lines() {
                        lines.push(Line::from(Span::styled(
                            format!("✗ {line}"),
                            theme.tool_error_style(),
                        )));
                    }
                }
                UiMessage::TurnComplete => {
                    flush_md(&mut lines, &mut md_buf, width);
                    flush_think(&mut lines, &mut think_buf, &theme);
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "─────",
                        Style::default().fg(theme.thinking_fg),
                    )));
                    lines.push(Line::from(""));
                }
            }
        }

        // Flush remaining buffers at end of messages.
        flush_md(&mut lines, &mut md_buf, width);
        flush_think(&mut lines, &mut think_buf, &theme);

        Text::from(lines)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, messages: &[UiMessage]) {
        let width = area.width as usize;
        let text = self.render_messages(messages, width);
        let total_lines = text.lines.len();
        let area_height = area.height as usize;
        let visible_start =
            total_lines.saturating_sub(area_height).saturating_sub(self.scroll_offset);
        let visible_end = total_lines.saturating_sub(self.scroll_offset);
        let visible_start = visible_start.min(visible_end.saturating_sub(area_height));

        let visible_lines: Vec<Line> =
            text.lines.into_iter().skip(visible_start).take(area_height).collect();

        let paragraph = Paragraph::new(Text::from(visible_lines)).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }
}

impl Default for ChatPanel {
    fn default() -> Self {
        Self::new()
    }
}
