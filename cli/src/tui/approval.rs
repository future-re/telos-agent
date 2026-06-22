//! Approval overlay — rendered on top of the base UI when the agent
//! requests permission for a command or file change.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::tui::render::Renderable;

/// An approval request that needs user input.
pub struct ApprovalRequest {
    pub tool_name: String,
    pub detail: String,
    pub reason: String,
}

pub struct ApprovalOverlay {
    pub request: Option<ApprovalRequest>,
    pub remember: bool,
}

impl Default for ApprovalOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalOverlay {
    pub fn new() -> Self {
        Self { request: None, remember: false }
    }

    pub fn is_visible(&self) -> bool {
        self.request.is_some()
    }

    pub fn handle_key(&mut self, c: char) -> Option<ApprovalDecision> {
        match c.to_ascii_lowercase() {
            'a' | 'y' => {
                let decision = ApprovalDecision::Allow;
                self.request = None;
                Some(decision)
            }
            'd' | 'n' => {
                let decision = ApprovalDecision::Deny;
                self.request = None;
                Some(decision)
            }
            'r' => {
                self.remember = !self.remember;
                None
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    Allow,
    Deny,
}

impl Renderable for ApprovalOverlay {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let Some(ref req) = self.request else { return };

        // Center a popup in the available area.
        let popup_w = area.width.saturating_sub(10).clamp(40, 80);
        let popup_h = 10u16;
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(popup_w)) / 2,
            y: area.y + (area.height.saturating_sub(popup_h)) / 2,
            width: popup_w,
            height: popup_h,
        };

        Clear.render(popup, buf);

        let block = Block::default()
            .title(" Approval required ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(255, 220, 110)));

        let mut lines = vec![
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    req.tool_name.clone(),
                    Style::default().fg(Color::Rgb(255, 220, 110)).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                format!("  $ {}", req.detail),
                Style::default().fg(Color::Rgb(200, 200, 200)),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", req.reason),
                Style::default().fg(Color::Rgb(138, 150, 170)),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  [a/y] approve  [d/n] deny  [r] remember",
                Style::default().fg(Color::Rgb(138, 150, 170)),
            )),
        ];

        if self.remember {
            lines.push(Line::from(Span::styled(
                "  remember: current session",
                Style::default().fg(Color::Rgb(138, 150, 170)),
            )));
        }

        let text = Text::from(lines);
        let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
        paragraph.render(popup, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        if self.request.is_some() { 10 } else { 0 }
    }
}
