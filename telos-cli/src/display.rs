use termimad::MadSkin;

/// Render markdown text as ANSI terminal output.
///
/// When `markdown_enabled` is true, the text is rendered through
/// termimad to produce ANSI escape codes for bold, headers, code, etc.
/// When false, the plain text is returned unchanged.
pub fn render(text: &str, markdown_enabled: bool) -> String {
    if !markdown_enabled {
        return text.to_string();
    }
    let skin = MadSkin::default();
    skin.term_text(text).to_string()
}

/// Render a diff between two strings with ANSI coloring.
///
/// Uses the `dissimilar` crate to compute the diff chunks:
/// - `Equal` chunks are left uncolored.
/// - `Delete` chunks are wrapped in red ANSI codes (`\x1b[31m`).
/// - `Insert` chunks are wrapped in green ANSI codes (`\x1b[32m`).
pub fn render_diff(old: &str, new: &str) -> String {
    let chunks = dissimilar::diff(old, new);
    let mut out = String::new();
    for chunk in chunks {
        match chunk {
            dissimilar::Chunk::Equal(s) => out.push_str(s),
            dissimilar::Chunk::Delete(s) => {
                out.push_str("\x1b[31m");
                out.push_str(s);
                out.push_str("\x1b[0m");
            }
            dissimilar::Chunk::Insert(s) => {
                out.push_str("\x1b[32m");
                out.push_str(s);
                out.push_str("\x1b[0m");
            }
        }
    }
    out
}
