use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, status: &str) {
    let theme = Theme::default();
    let style = Style::default().fg(theme.status_fg).bg(theme.status_bg);

    // Split status into segments on " · " for visual styling.
    let parts: Vec<&str> = status.split(" · ").collect();
    let mut spans = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                " · ",
                Style::default().fg(theme.thinking_fg).bg(theme.status_bg),
            ));
        }
        let s = if i == 0 {
            // First segment (app name) is bold.
            Style::default().fg(theme.status_fg).bg(theme.status_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.status_fg).bg(theme.status_bg)
        };
        spans.push(Span::styled(part.to_string(), s));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(style);
    frame.render_widget(paragraph, area);
}
