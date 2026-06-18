use ratatui::style::{Color, Style};

#[derive(Debug, Clone, Copy, Default)]
pub struct Theme;
impl Theme {
    pub fn user_style(&self) -> Style {
        Style::default().fg(Color::Cyan)
    }
    pub fn assistant_style(&self) -> Style {
        Style::default().fg(Color::Gray)
    }
}
