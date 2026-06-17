//! Recursive analysis of command substitutions.
//!
//! When a command contains `$(inner)` or backticks, the inner command is itself
//! a shell command that should be analyzed. This module extracts those inner
//! commands and recursively classifies them.

use crate::bash_security::parser::Node;
use crate::bash_security::{CommandSafety, analyze as analyze_command};

/// Result of analyzing all command substitutions inside a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubstitutionAnalysis {
    /// No command substitutions found.
    None,
    /// All inner commands are safe.
    Safe,
    /// At least one inner command needs review.
    NeedsReview { reason: String },
}

/// Analyze every command substitution inside `node`.
///
/// The `source` string is the original command text, used to slice the inner
/// command text by byte offsets.
pub fn analyze_substitutions(node: &Node, _source: &str) -> SubstitutionAnalysis {
    let mut inner: Vec<String> = Vec::new();
    collect_substitution_texts(node, &mut inner);

    if inner.is_empty() {
        return SubstitutionAnalysis::None;
    }

    for text in inner {
        match analyze_command(&text) {
            CommandSafety::Safe => {}
            CommandSafety::NeedsReview { reason } => {
                return SubstitutionAnalysis::NeedsReview {
                    reason: format!("inner command `{text}` needs review: {reason}"),
                };
            }
        }
    }

    SubstitutionAnalysis::Safe
}

fn collect_substitution_texts(node: &Node, out: &mut Vec<String>) {
    if node.kind == "command_substitution" {
        // Body is everything between $( and ).
        let body = if node.text.starts_with("$(") && node.text.ends_with(')') {
            &node.text[2..node.text.len() - 1]
        } else if node.text.starts_with('`') && node.text.ends_with('`') {
            &node.text[1..node.text.len() - 1]
        } else {
            &node.text
        };
        out.push(body.to_string());
        // Do NOT recurse into the substitution body — analyze_command will
        // parse it separately.
        return;
    }

    for child in &node.children {
        collect_substitution_texts(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash_security::parser;

    #[test]
    fn safe_inner_command_allowed() {
        let ast = parser::parse("echo $(git status)").unwrap();
        let result = analyze_substitutions(&ast, "echo $(git status)");
        assert_eq!(result, SubstitutionAnalysis::Safe);
    }

    #[test]
    fn dangerous_inner_command_rejected() {
        let ast = parser::parse("echo $(rm -rf /)").unwrap();
        let result = analyze_substitutions(&ast, "echo $(rm -rf /)");
        assert!(matches!(result, SubstitutionAnalysis::NeedsReview { .. }), "expected review");
    }

    #[test]
    fn no_substitution() {
        let ast = parser::parse("echo hello").unwrap();
        let result = analyze_substitutions(&ast, "echo hello");
        assert_eq!(result, SubstitutionAnalysis::None);
    }
}
