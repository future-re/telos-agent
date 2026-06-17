//! Redirect analysis and validation.
//!
//! Extracts output/input redirects from a bash AST and decides whether each
//! target is a static path (safe to validate against the workspace) or a
//! dynamic path (contains variables, globs, command substitution, etc.).

use std::path::Path;

use crate::bash_security::parser::{self, Node};
use crate::error::AgentError;
use crate::tools::resolve_workspace_path;

/// A redirect that has been extracted from the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub op: parser::RedirectOp,
    pub target: String,
    pub fd: Option<u32>,
    /// True if the target contains no shell expansions.
    pub is_static: bool,
}

impl Redirect {
    /// Validate a static output redirect against the workspace cwd.
    ///
    /// Returns `Ok(())` if the target lies inside `cwd`, otherwise an error.
    pub fn validate_static_output(&self, cwd: &Path) -> Result<(), AgentError> {
        if !self.op.is_output() {
            return Ok(());
        }
        if !self.is_static {
            return Err(AgentError::PermissionDenied(
                "dynamic redirect target".into(),
            ));
        }
        resolve_workspace_path(cwd, &self.target)?;
        Ok(())
    }

    /// True for FD-to-FD redirects such as `2>&1`, `1>&2`, or `>&-`.
    ///
    /// These do not name a file path and are therefore safe from path-based
    /// injection; they only rearrange stdout/stderr file descriptors.
    pub fn is_fd_redirect(&self) -> bool {
        matches!(self.op, parser::RedirectOp::DupOutput | parser::RedirectOp::DupInput)
            && (self.target == "-" || self.target.parse::<u32>().is_ok())
    }
}

/// Extract all redirects from a `redirected_statement` node.
pub fn extract_redirects(node: &Node) -> Vec<Redirect> {
    if node.kind != "redirected_statement" {
        return Vec::new();
    }
    node.children
        .iter()
        .filter(|c| c.kind == "file_redirect")
        .filter_map(extract_single_redirect)
        .collect()
}

fn extract_single_redirect(node: &Node) -> Option<Redirect> {
    let mut fd: Option<u32> = None;
    let mut op_kind: Option<&str> = None;
    let mut target: Option<String> = None;

    for child in &node.children {
        match child.kind.as_str() {
            kind if parser::redirect_op_from_kind(kind).is_some() => {
                op_kind = Some(kind);
                // Closing redirects like `>&-` carry their target in the token.
                if kind.ends_with('-') {
                    target = Some("-".to_string());
                }
            }
            "file_descriptor" => {
                if op_kind.is_none() && let Ok(n) = child.text.parse::<u32>() {
                    fd = Some(n);
                }
            }
            "word" | "string" | "raw_string" | "number" => {
                if op_kind.is_none() {
                    if let Ok(n) = child.text.parse::<u32>() {
                        fd = Some(n);
                    }
                } else {
                    target = Some(strip_quotes(&child.text));
                }
            }
            _ => {}
        }
    }

    let op = parser::redirect_op_from_kind(op_kind?)?;
    let target = target.unwrap_or_default();
    let is_static = is_static_target(&target);

    Some(Redirect {
        op,
        target,
        fd,
        is_static,
    })
}

/// A static redirect target in bash is a single shell word with no expansion.
///
/// Rejects variables, command substitution, globs, brace expansion, tilde,
/// history expansion, zsh equals expansion, and empty strings.
pub fn is_static_target(target: &str) -> bool {
    if target.is_empty() {
        return false;
    }
    if target.contains(|c: char| c.is_whitespace()) {
        return false;
    }

    let forbidden_starts = ['!', '='];
    if forbidden_starts.iter().any(|c| target.starts_with(*c)) {
        return false;
    }

    let forbidden_chars = ['$', '`', '*', '?', '[', '{', '}', '~', '<', '>', '(', ')'];
    if target.contains(forbidden_chars) {
        return false;
    }

    true
}

fn strip_quotes(text: &str) -> String {
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash_security::parser;

    #[test]
    fn detects_static_vs_dynamic_targets() {
        assert!(is_static_target("file.txt"));
        assert!(is_static_target("/tmp/output"));
        assert!(!is_static_target(""));
        assert!(!is_static_target("$HOME"));
        assert!(!is_static_target("`cmd`"));
        assert!(!is_static_target("*.txt"));
        assert!(!is_static_target("{a,b}"));
        assert!(!is_static_target("~/.bashrc"));
        assert!(!is_static_target("=cmd"));
    }

    #[test]
    fn extracts_output_redirect() {
        let ast = parser::parse("echo hello > file.txt").unwrap();
        let redirects = extract_redirects(ast.find_descendant("redirected_statement").unwrap());
        assert_eq!(redirects.len(), 1);
        assert!(redirects[0].op.is_output());
        assert_eq!(redirects[0].target, "file.txt");
        assert!(redirects[0].is_static);
    }

    #[test]
    fn fd_redirects_are_identified() {
        let ast = parser::parse("git status 2>&1").unwrap();
        let redirects = extract_redirects(ast.find_descendant("redirected_statement").unwrap());
        assert_eq!(redirects.len(), 1);
        assert!(redirects[0].is_fd_redirect());
        assert_eq!(redirects[0].fd, Some(2));
        assert_eq!(redirects[0].target, "1");

        let ast = parser::parse("cmd >&-").unwrap();
        let redirects = extract_redirects(ast.find_descendant("redirected_statement").unwrap());
        assert_eq!(redirects.len(), 1);
        assert!(redirects[0].is_fd_redirect());
        assert_eq!(redirects[0].target, "-");
    }

    #[test]
    fn file_dup_redirects_are_not_fd_redirects() {
        let ast = parser::parse("cmd &> file.log").unwrap();
        let redirects = extract_redirects(ast.find_descendant("redirected_statement").unwrap());
        assert_eq!(redirects.len(), 1);
        assert!(!redirects[0].is_fd_redirect());
    }
}
