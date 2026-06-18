use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, status: &str) {
    let style = Style::default().fg(Color::White).bg(Color::DarkGray);
    let paragraph = Paragraph::new(Line::from(status)).style(style);
    frame.render_widget(paragraph, area);
}
