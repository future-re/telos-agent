use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

/// Render markdown text as ratatui `Text`.
///
/// We use termimad to produce ANSI-colored output, then strip the ANSI escape
/// sequences and map a subset of styles to ratatui spans. This is a pragmatic
/// first pass; a native ratatui markdown parser can replace it later.
pub fn render_markdown(input: &str) -> Text<'static> {
    let skin = termimad::MadSkin::default();
    let fmt_text = skin.text(input, None);
    let rendered_string = fmt_text.to_string();

    let mut lines: Vec<Line> = Vec::new();
    for raw_line in rendered_string.lines() {
        let (line, _) = strip_ansi_and_build_spans(raw_line);
        lines.push(line);
    }

    Text::from(lines)
}

/// Naively strip ANSI escapes and track the active style.
///
/// Returns a ratatui `Line` plus the style that was active at the end of the
/// line (useful if a span wraps across lines, though termimad normally closes
/// escapes at line boundaries).
fn strip_ansi_and_build_spans(line: &str) -> (Line<'static>, Style) {
    let mut spans: Vec<Span> = Vec::new();
    let mut current_text = String::new();
    let mut current_style = Style::default();

    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            // Flush current span before processing escape.
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }
            // Read escape sequence up to 'm'.
            chars.next(); // consume '['
            let mut seq = String::new();
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
                seq.push(c);
            }
            current_style = apply_ansi_sgr(&seq, current_style);
        } else {
            current_text.push(ch);
        }
    }

    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    if spans.is_empty() {
        spans.push(Span::from(""));
    }

    (Line::from(spans), current_style)
}

fn apply_ansi_sgr(seq: &str, base: Style) -> Style {
    let mut style = base;
    for code in seq.split(';') {
        match code {
            "0" => style = Style::default(),
            "1" => style = style.add_modifier(Modifier::BOLD),
            "3" => style = style.add_modifier(Modifier::ITALIC),
            "4" => style = style.add_modifier(Modifier::UNDERLINED),
            "22" => style = style.remove_modifier(Modifier::BOLD),
            "23" => style = style.remove_modifier(Modifier::ITALIC),
            "24" => style = style.remove_modifier(Modifier::UNDERLINED),
            "31" => style = style.fg(Color::Red),
            "32" => style = style.fg(Color::Green),
            "33" => style = style.fg(Color::Yellow),
            "34" => style = style.fg(Color::Blue),
            "35" => style = style.fg(Color::Magenta),
            "36" => style = style.fg(Color::Cyan),
            "90" => style = style.fg(Color::DarkGray),
            _ => {}
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_bold_text() {
        let text = render_markdown("**hello**");
        assert!(!text.lines.is_empty());
        let first = text.lines[0].spans.clone();
        assert!(first.iter().any(|s| s.content.contains("hello")));
    }
}
