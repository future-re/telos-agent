use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
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

    fn render_messages(&self, messages: &[UiMessage]) -> Text<'static> {
        let theme = Theme::default();
        let mut lines: Vec<Line> = Vec::new();

        for msg in messages {
            match msg {
                UiMessage::User(content) => {
                    for line in content.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("▸ ", theme.user_style()),
                            Span::styled(line.to_string(), theme.user_style()),
                        ]));
                    }
                }
                UiMessage::AssistantDelta(text) => {
                    // Append to the last assistant line if possible.
                    if let Some(last) = lines.last_mut()
                        && last.spans.len() == 1
                        && last.spans[0].style == theme.assistant_style()
                        && !text.contains('\n')
                    {
                        last.spans[0].content = format!("{}{}", last.spans[0].content, text).into();
                    } else {
                        for line in text.lines() {
                            lines.push(Line::from(Span::styled(
                                line.to_string(),
                                theme.assistant_style(),
                            )));
                        }
                    }
                }
                UiMessage::ThinkingDelta(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            theme.thinking_style(),
                        )));
                    }
                }
                UiMessage::ToolCall { name, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("  ⏳ ", theme.tool_pending_style()),
                        Span::styled(name.clone(), theme.tool_pending_style()),
                    ]));
                }
                UiMessage::ToolCompleted { name, is_error, .. } => {
                    let (icon, style) = if *is_error {
                        ("  ✗ ", theme.tool_error_style())
                    } else {
                        ("  ✓ ", theme.tool_ok_style())
                    };
                    lines.push(Line::from(vec![
                        Span::styled(icon, style),
                        Span::styled(name.clone(), style),
                    ]));
                }
                UiMessage::TurnComplete => {
                    lines.push(Line::from(Span::styled(
                        "───",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
        }

        Text::from(lines)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, messages: &[UiMessage]) {
        let text = self.render_messages(messages);
        let total_lines = text.lines.len();
        let area_height = area.height as usize;
        let visible_start =
            total_lines.saturating_sub(area_height).saturating_sub(self.scroll_offset);
        let visible_end = total_lines.saturating_sub(self.scroll_offset);
        let visible_start = visible_start.min(visible_end.saturating_sub(area_height));

        let visible_lines: Vec<Line> =
            text.lines.into_iter().skip(visible_start).take(area_height).collect();

        let paragraph = Paragraph::new(Text::from(visible_lines)).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }
}

impl Default for ChatPanel {
    fn default() -> Self {
        Self::new()
    }
}
