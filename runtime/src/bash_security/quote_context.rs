//! Quote context extraction for bash commands.
//!
//! Provides three views of a command string, matching the semantics of
//! telos-agent's `QuoteContext` in `utils/bash/treeSitterAnalysis.ts`:
//!
//! - `fully_unquoted`: all quoted content removed
//! - `with_double_quotes`: single-quoted / ANSI-C / heredoc content removed,
//!   double-quoted content preserved but delimiters stripped
//! - `unquoted_keep_quote_chars`: quoted content removed, but quote delimiters kept
//!
//! These views are used to decide whether `$()`/`${}` / dangerous patterns are
//! inside a context where they would actually expand at runtime.

use super::parser::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteContext {
    /// Content with single-quoted / ANSI-C / heredoc content removed and
    /// double-quote delimiters stripped (content preserved).
    pub with_double_quotes: String,
    /// Content with all quoted spans removed.
    pub fully_unquoted: String,
    /// Content with quoted spans removed, but delimiters (`'`, `"`, `$'`) kept.
    pub unquoted_keep_quote_chars: String,
}

impl QuoteContext {
    /// Extract quote context from a command AST.
    pub fn from_node(root: &Node, source: &str) -> Self {
        let spans = collect_quote_spans(root);

        let single_quote_set: std::collections::HashSet<usize> = spans
            .iter()
            .filter(|s| s.kind != SpanKind::Double)
            .flat_map(|s| s.start..s.end)
            .collect();

        let double_quote_delims: std::collections::HashSet<usize> = spans
            .iter()
            .filter(|s| s.kind == SpanKind::Double)
            .flat_map(|s| [s.start, s.end - 1])
            .collect();

        let mut with_double_quotes = String::with_capacity(source.len());
        for (i, c) in source.char_indices() {
            if single_quote_set.contains(&i) {
                continue;
            }
            if double_quote_delims.contains(&i) {
                continue;
            }
            with_double_quotes.push(c);
        }

        let fully_unquoted = remove_spans(source, &spans);
        let unquoted_keep_quote_chars = replace_spans_keep_quotes(source, &spans);

        Self { with_double_quotes, fully_unquoted, unquoted_keep_quote_chars }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpanKind {
    Single,
    Double,
    AnsiC,
    Heredoc,
}

#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    kind: SpanKind,
}

fn collect_quote_spans(node: &Node) -> Vec<Span> {
    let mut out = Vec::new();
    collect_quote_spans_inner(node, &mut out, false);
    out
}

fn collect_quote_spans_inner(node: &Node, out: &mut Vec<Span>, in_double: bool) {
    match node.kind.as_str() {
        "raw_string" => {
            out.push(Span { start: node.start_byte, end: node.end_byte, kind: SpanKind::Single });
            return;
        }
        "ansi_c_string" => {
            out.push(Span { start: node.start_byte, end: node.end_byte, kind: SpanKind::AnsiC });
            return;
        }
        "string" => {
            if !in_double {
                out.push(Span {
                    start: node.start_byte,
                    end: node.end_byte,
                    kind: SpanKind::Double,
                });
            }
            for child in &node.children {
                collect_quote_spans_inner(child, out, true);
            }
            return;
        }
        "heredoc_redirect" if is_quoted_heredoc(node) => {
            out.push(Span { start: node.start_byte, end: node.end_byte, kind: SpanKind::Heredoc });
            return;
        }
        _ => {}
    }

    for child in &node.children {
        collect_quote_spans_inner(child, out, in_double);
    }
}

fn is_quoted_heredoc(node: &Node) -> bool {
    node.children
        .iter()
        .find(|c| c.kind == "heredoc_start")
        .map(|c| {
            let first = c.text.chars().next().unwrap_or('\0');
            first == '\'' || first == '"' || first == '\\'
        })
        .unwrap_or(false)
}

/// Drop spans fully contained within another span, keeping only the outermost.
fn drop_contained_spans(spans: &mut Vec<Span>) {
    let mut keep = vec![true; spans.len()];
    for (i, s) in spans.iter().enumerate() {
        for (j, other) in spans.iter().enumerate() {
            if i == j {
                continue;
            }
            if other.start <= s.start
                && other.end >= s.end
                && (other.start < s.start || other.end > s.end)
            {
                keep[i] = false;
                break;
            }
        }
    }
    let mut i = 0;
    spans.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
}

fn remove_spans(source: &str, spans: &[Span]) -> String {
    if spans.is_empty() {
        return source.to_string();
    }
    let mut sorted: Vec<Span> = spans.to_vec();
    drop_contained_spans(&mut sorted);
    sorted.sort_by_key(|s| s.start);

    let mut result = String::with_capacity(source.len());
    let mut last_end = 0usize;
    for span in sorted {
        result.push_str(&source[last_end..span.start.min(source.len())]);
        last_end = span.end.min(source.len());
    }
    result.push_str(&source[last_end..]);
    result
}

fn replace_spans_keep_quotes(source: &str, spans: &[Span]) -> String {
    if spans.is_empty() {
        return source.to_string();
    }
    let sorted: Vec<(usize, usize, &'static str, &'static str)> = spans
        .iter()
        .map(|s| {
            let (open, close) = match s.kind {
                SpanKind::Single => ("'", "'"),
                SpanKind::Double => ("\"", "\""),
                SpanKind::AnsiC => ("$'", "'"),
                SpanKind::Heredoc => ("", ""),
            };
            (s.start, s.end, open, close)
        })
        .collect();

    // Drop contained spans
    let len = sorted.len();
    let mut keep = vec![true; len];
    for i in 0..len {
        for j in 0..len {
            if i == j {
                continue;
            }
            let (s_start, s_end, _, _) = sorted[i];
            let (o_start, o_end, _, _) = sorted[j];
            if o_start <= s_start && o_end >= s_end && (o_start < s_start || o_end > s_end) {
                keep[i] = false;
                break;
            }
        }
    }
    let mut filtered = Vec::new();
    for i in 0..len {
        if keep[i] {
            filtered.push(sorted[i]);
        }
    }

    filtered.sort_by_key(|s| s.0);

    let mut result = String::with_capacity(source.len());
    let mut last_end = 0usize;
    for (start, end, open, close) in filtered {
        result.push_str(&source[last_end..start.min(source.len())]);
        result.push_str(open);
        result.push_str(close);
        last_end = end.min(source.len());
    }
    result.push_str(&source[last_end..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash_security::parser;

    fn ctx(cmd: &str) -> QuoteContext {
        let ast = parser::parse(cmd).unwrap();
        QuoteContext::from_node(&ast, cmd)
    }

    #[test]
    fn removes_single_quoted_content() {
        let c = ctx("echo 'hello world'");
        assert_eq!(c.fully_unquoted, "echo ");
        assert_eq!(c.with_double_quotes, "echo ");
        assert_eq!(c.unquoted_keep_quote_chars, "echo ''");
    }

    #[test]
    fn keeps_double_quoted_content() {
        let c = ctx(r#"echo "hello world""#);
        assert_eq!(c.fully_unquoted, "echo ");
        assert_eq!(c.with_double_quotes, "echo hello world");
        assert_eq!(c.unquoted_keep_quote_chars, "echo \"\"");
    }

    #[test]
    fn nested_quotes() {
        let c = ctx(r#"echo "$(echo 'hi')""#);
        // Outer double quotes removed, inner single-quoted content removed.
        assert_eq!(c.with_double_quotes, "echo $(echo )");
    }
}
