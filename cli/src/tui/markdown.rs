//! Markdown → ratatui [`Text`] rendering via [`ratatui_markdown`].
//!
//! Uses the native ratatui renderer so there is no ANSI-roundtrip and
//! code blocks get syntax highlighting out of the box.

use ratatui::text::{Line, Text};

/// Render a markdown string into ratatui `Text`, wrapping at `width` columns.
///
/// The returned `Text` is ready to be displayed in a [`Paragraph`] widget.
/// Code blocks tagged with a language (e.g. ` ```rust `) are syntax-highlighted
/// using tree-sitter grammars bundled with `ratatui-markdown`.
pub fn render_markdown(input: &str, width: usize) -> Text<'static> {
    use ratatui_markdown::markdown::MarkdownRenderer;
    use ratatui_markdown::theme::ThemeConfig;

    // Clamp to a reasonable minimum so the renderer doesn't panic on tiny
    // terminal sizes.
    let w = width.max(20);

    let renderer = MarkdownRenderer::new(w);
    let blocks = renderer.parse(input);
    let lines: Vec<Line<'static>> = renderer.render(&blocks, &ThemeConfig::default());

    Text::from(lines)
}
