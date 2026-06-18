use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub status_bg: Color,
    pub status_fg: Color,
    pub user_fg: Color,
    pub assistant_fg: Color,
    pub tool_pending_fg: Color,
    pub tool_ok_fg: Color,
    pub tool_error_fg: Color,
    pub thinking_fg: Color,
    pub border_active: Color,
    pub border_inactive: Color,
    pub input_placeholder: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            status_bg: Color::Rgb(30, 40, 60),
            status_fg: Color::Rgb(200, 210, 230),
            user_fg: Color::Rgb(100, 200, 255),
            assistant_fg: Color::Rgb(220, 220, 230),
            tool_pending_fg: Color::Rgb(255, 200, 80),
            tool_ok_fg: Color::Rgb(100, 220, 120),
            tool_error_fg: Color::Rgb(255, 100, 100),
            thinking_fg: Color::Rgb(100, 100, 120),
            border_active: Color::Rgb(100, 200, 255),
            border_inactive: Color::Rgb(60, 60, 80),
            input_placeholder: Color::Rgb(80, 80, 100),
        }
    }
}

impl Theme {
    pub fn user_style(&self) -> Style {
        Style::default().fg(self.user_fg).add_modifier(Modifier::BOLD)
    }

    pub fn assistant_style(&self) -> Style {
        Style::default().fg(self.assistant_fg)
    }

    pub fn thinking_style(&self) -> Style {
        Style::default().fg(self.thinking_fg).add_modifier(Modifier::ITALIC)
    }

    pub fn tool_pending_style(&self) -> Style {
        Style::default().fg(self.tool_pending_fg)
    }

    pub fn tool_ok_style(&self) -> Style {
        Style::default().fg(self.tool_ok_fg)
    }

    pub fn tool_error_style(&self) -> Style {
        Style::default().fg(self.tool_error_fg)
    }
}
