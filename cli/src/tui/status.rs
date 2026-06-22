//! Status bar at the bottom of the screen.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::tui::render::Renderable;

pub const SPINNER_CHARS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

pub struct StatusBar {
    pub status_text: String,
    pub spinner_frame: usize,
    pub token_budget_pct: Option<f64>,
}

impl StatusBar {
    pub fn new(status_text: String) -> Self {
        Self { status_text, spinner_frame: 0, token_budget_pct: None }
    }
}

impl Renderable for StatusBar {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let bg = Color::Rgb(12, 18, 26);
        let accent = Style::default().fg(Color::Rgb(115, 220, 255)).bg(bg);
        let primary = Style::default().fg(Color::Rgb(232, 238, 248)).bg(bg);
        let muted = Style::default().fg(Color::Rgb(138, 150, 170)).bg(bg);

        let spinner = SPINNER_CHARS[self.spinner_frame % SPINNER_CHARS.len()];

        let mut spans = vec![Span::styled(format!(" {} ", spinner), accent)];

        // Parse "telos · part1 · part2" and render badge + parts
        let parts: Vec<&str> = self.status_text.split(" · ").collect();
        if parts.first().is_some_and(|p| p.eq_ignore_ascii_case("telos")) {
            spans.push(Span::styled(
                " TELOS ",
                accent
                    .bg(Color::Rgb(115, 220, 255))
                    .fg(Color::Rgb(12, 18, 26))
                    .add_modifier(Modifier::BOLD),
            ));
            for part in parts.iter().skip(1) {
                spans.push(Span::styled("  │  ", muted));
                spans.push(Span::styled(
                    part.trim().to_string(),
                    primary.add_modifier(Modifier::BOLD),
                ));
            }
        } else {
            spans.push(Span::styled(self.status_text.clone(), primary));
        }

        // Token budget bar
        if let Some(pct) = self.token_budget_pct {
            let bar_width = 10usize;
            let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
            let empty = bar_width.saturating_sub(filled);

            let bar_color = if pct >= 95.0 {
                Color::Rgb(255, 95, 95)
            } else if pct >= 90.0 {
                Color::Rgb(255, 176, 80)
            } else if pct >= 75.0 {
                Color::Rgb(255, 220, 110)
            } else {
                Color::Rgb(110, 220, 145)
            };

            spans.push(Span::styled("  │  ", muted));
            spans.push(Span::styled("tok ", muted.add_modifier(Modifier::DIM)));
            spans.push(Span::styled(
                format!(" {}{} {:3.0}% ", "█".repeat(filled), "░".repeat(empty), pct),
                Style::default().fg(bar_color).bg(Color::Rgb(18, 25, 34)),
            ));
        }

        // Fill the rest of the area
        let line = Line::from(spans);
        Paragraph::new(line)
            .style(Style::default().bg(bg).fg(Color::Rgb(232, 238, 248)))
            .render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}
