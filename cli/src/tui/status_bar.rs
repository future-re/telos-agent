use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme::Theme;

/// Braille spinner animation frames.
const SPINNER_CHARS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

/// Render the status bar with an animated spinner, status text, and an optional
/// token-usage progress bar.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    status: &str,
    spinner_frame: usize,
    tokens_used: u64,
    tokens_max: Option<u64>,
) {
    let theme = Theme::default();
    let mut spans: Vec<Span> = Vec::new();

    // ── Animated spinner ─────────────────────────────────────────
    let spinner_char = SPINNER_CHARS[spinner_frame % SPINNER_CHARS.len()];
    spans.push(Span::styled(
        format!(" {} ", spinner_char),
        Style::default().fg(theme.status_fg).bg(theme.status_bg),
    ));

    // ── Status text (split on " · " for visual styling) ──────────
    let parts: Vec<&str> = status.split(" · ").collect();
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

    // ── Token progress bar ──────────────────────────────────────
    if let Some(max) = tokens_max {
        if max > 0 && tokens_used > 0 {
            let pct = (tokens_used as f64 / max as f64 * 100.0).clamp(0.0, 100.0);
            let bar_width = 10usize;
            let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
            let empty = bar_width.saturating_sub(filled);
            let bar = format!("[{}{}] {:3.0}%", "█".repeat(filled), "░".repeat(empty), pct);

            let bar_color = if pct >= 95.0 {
                Color::Red
            } else if pct >= 90.0 {
                Color::Rgb(255, 165, 0) // orange
            } else if pct >= 75.0 {
                Color::Yellow
            } else {
                Color::Green
            };

            spans.push(Span::styled(" ", Style::default().bg(theme.status_bg)));
            spans.push(Span::styled(bar, Style::default().fg(bar_color).bg(theme.status_bg)));
        }
    }

    let style = Style::default().fg(theme.status_fg).bg(theme.status_bg);
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line).style(style), area);
}
