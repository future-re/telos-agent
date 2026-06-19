/// Available slash commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Tool,
    Model,
    Api,
    Help,
    Clear,
    Session,
    Auto,
}

impl SlashCommand {
    /// All registered commands with their metadata.
    pub fn registry() -> Vec<(&'static str, &'static str, SlashCommand)> {
        vec![
            ("tool", "List or configure tools", SlashCommand::Tool),
            ("model", "Switch model provider", SlashCommand::Model),
            ("api", "Set DeepSeek API key", SlashCommand::Api),
            ("help", "Show help information", SlashCommand::Help),
            ("clear", "Clear conversation", SlashCommand::Clear),
            ("session", "Session management", SlashCommand::Session),
            ("auto", "Toggle auto-approve mode", SlashCommand::Auto),
        ]
    }

    /// Filter commands matching a prefix.
    pub fn matching(prefix: &str) -> Vec<(&'static str, &'static str, SlashCommand)> {
        let lower = prefix.to_lowercase();
        Self::registry().into_iter().filter(|(name, _, _)| name.starts_with(&lower)).collect()
    }
}

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::theme::Theme;

pub struct CommandPopup {
    /// Current filter text (everything after /)
    filter: String,
    /// Matched commands
    matches: Vec<(&'static str, &'static str, SlashCommand)>,
    /// Selected index in matches
    selected: usize,
    /// Whether the popup is visible
    pub visible: bool,
}

impl CommandPopup {
    pub fn new() -> Self {
        Self { filter: String::new(), matches: Vec::new(), selected: 0, visible: false }
    }

    /// Show popup after `/` was typed.
    pub fn show(&mut self) {
        self.filter.clear();
        self.matches = SlashCommand::matching("");
        self.selected = 0;
        self.visible = true;
    }

    /// Hide popup.
    pub fn hide(&mut self) {
        self.visible = false;
        self.filter.clear();
        self.matches.clear();
        self.selected = 0;
    }

    /// Update filter text.
    pub fn update_filter(&mut self, text: &str) {
        self.filter = text.to_string();
        self.matches = SlashCommand::matching(text);
        self.selected = 0;
    }

    /// Navigate selection.
    pub fn select_next(&mut self) {
        if !self.matches.is_empty() {
            self.selected = (self.selected + 1) % self.matches.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.matches.is_empty() {
            self.selected =
                if self.selected == 0 { self.matches.len() - 1 } else { self.selected - 1 };
        }
    }

    /// Get the currently selected command (if any).
    pub fn selected_command(&self) -> Option<&SlashCommand> {
        self.matches.get(self.selected).map(|(_, _, cmd)| cmd)
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible || self.matches.is_empty() {
            return;
        }

        let popup_width = area.width.min(56);
        let popup_height = (self.matches.len() as u16).min(10) + 2; // border
        let popup_x = area.x;
        let popup_y = area.y.saturating_sub(popup_height + 1);

        let popup_area = Rect { x: popup_x, y: popup_y, width: popup_width, height: popup_height };

        let block = Block::default()
            .title(" Commands ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active));

        let mut lines = Vec::new();
        for (i, (name, desc, _)) in self.matches.iter().enumerate() {
            let style = if i == self.selected {
                Style::default().fg(theme.border_active).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme.assistant_fg)
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" /{:<12}", name), style),
                Span::styled(*desc, Style::default().fg(theme.thinking_fg)),
            ]));
        }

        let paragraph = Paragraph::new(Text::from(lines)).block(block);
        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }
}

impl Default for CommandPopup {
    fn default() -> Self {
        Self::new()
    }
}
