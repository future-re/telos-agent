use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::tui::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, status: &str) {
    let theme = Theme::default();
    let style = Style::default().fg(theme.status_fg).bg(theme.status_bg);
    let paragraph = Paragraph::new(Line::from(status.to_string())).style(style);
    frame.render_widget(paragraph, area);
}
