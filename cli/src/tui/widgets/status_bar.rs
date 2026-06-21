use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme::Theme;

/// Braille spinner animation frames.
pub const SPINNER_CHARS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
const STATUS_RESERVE: usize = 20;

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

    let bg = theme.status_bg;
    let primary = Style::default().fg(Color::Rgb(232, 238, 248)).bg(bg);
    let muted = Style::default().fg(Color::Rgb(138, 150, 170)).bg(bg);
    let accent = Style::default().fg(Color::Rgb(115, 220, 255)).bg(bg).add_modifier(Modifier::BOLD);
    let badge = Style::default()
        .fg(Color::Rgb(12, 18, 26))
        .bg(Color::Rgb(115, 220, 255))
        .add_modifier(Modifier::BOLD);

    let spinner_char = SPINNER_CHARS[spinner_frame % SPINNER_CHARS.len()];
    spans.push(Span::styled(format!(" {} ", spinner_char), accent));

    let available = area.width as usize;
    let status_budget = available.saturating_sub(STATUS_RESERVE).max(12);
    let status = truncate_chars(status, status_budget);

    let parts: Vec<&str> = status.split(" · ").collect();
    if parts.first().is_some_and(|part| part.eq_ignore_ascii_case("telos")) {
        spans.push(Span::styled(" TELOS ", badge));
        for part in parts.iter().skip(1) {
            push_separator(&mut spans, muted);
            push_status_text(&mut spans, part, primary, muted, accent);
        }
    } else {
        push_status_text(&mut spans, &status, primary, muted, accent);
    }

    if let Some(max) = tokens_max
        && max > 0
        && tokens_used > 0
    {
        let pct = (tokens_used as f64 / max as f64 * 100.0).clamp(0.0, 100.0);
        let bar_width = 10usize;
        let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
        let empty = bar_width.saturating_sub(filled);
        let bar = format!(" {}{} {:3.0}% ", "█".repeat(filled), "░".repeat(empty), pct);

        let bar_color = if pct >= 95.0 {
            Color::Rgb(255, 95, 95)
        } else if pct >= 90.0 {
            Color::Rgb(255, 176, 80)
        } else if pct >= 75.0 {
            Color::Rgb(255, 220, 110)
        } else {
            Color::Rgb(110, 220, 145)
        };

        push_separator(&mut spans, muted);
        spans.push(Span::styled("tok ", muted.add_modifier(Modifier::DIM)));
        spans.push(Span::styled(bar, Style::default().fg(bar_color).bg(Color::Rgb(18, 25, 34))));
    }

    spans.push(Span::styled(" ".repeat(area.width as usize), Style::default().bg(bg)));

    let style = Style::default().fg(theme.status_fg).bg(bg);
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line).style(style), area);
}

fn push_separator(spans: &mut Vec<Span<'static>>, style: Style) {
    spans.push(Span::styled("  │  ", style));
}

fn push_status_text(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    primary: Style,
    muted: Style,
    accent: Style,
) {
    let trimmed = text.trim();
    if let Some((main, meta)) = split_metadata(trimmed) {
        spans.push(Span::styled(main.to_string(), accent));
        spans.push(Span::styled(" ".to_string(), muted));
        spans.push(Span::styled(meta.to_string(), muted.add_modifier(Modifier::DIM)));
    } else {
        spans.push(Span::styled(trimmed.to_string(), primary.add_modifier(Modifier::BOLD)));
    }
}

fn split_metadata(text: &str) -> Option<(&str, &str)> {
    let open = text.rfind(" (")?;
    text.ends_with(')').then_some((&text[..open], &text[open + 1..]))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", text.chars().take(keep).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::{split_metadata, truncate_chars};

    #[test]
    fn split_metadata_detects_parenthesized_suffix() {
        assert_eq!(split_metadata("streaming (2s | tok)"), Some(("streaming", "(2s | tok)")));
    }

    #[test]
    fn truncate_chars_preserves_utf8_boundaries() {
        assert_eq!(truncate_chars("状态栏测试", 4), "状态栏…");
    }
}
