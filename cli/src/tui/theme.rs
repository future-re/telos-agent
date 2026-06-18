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
            status_bg: Color::DarkGray,
            status_fg: Color::White,
            user_fg: Color::Cyan,
            assistant_fg: Color::Gray,
            tool_pending_fg: Color::Yellow,
            tool_ok_fg: Color::Green,
            tool_error_fg: Color::Red,
            thinking_fg: Color::DarkGray,
            border_active: Color::Cyan,
            border_inactive: Color::DarkGray,
            input_placeholder: Color::DarkGray,
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
