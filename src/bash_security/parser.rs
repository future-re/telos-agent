//! Bash AST parser wrapper around `tree-sitter-bash`.
//!
//! Converts tree-sitter's raw [`tree_sitter::Node`] graph into a friendlier
//! [`Node`] tree that the security analyzer consumes. All offsets are UTF-8
//! byte offsets and [`Node::text`] is sliced directly from the original source
//! so callers never have to manage tree-sitter lifetimes themselves.

use std::collections::HashSet;

/// A node in the bash AST.
#[derive(Debug, Clone)]
pub struct Node {
    pub kind: String,
    pub text: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub children: Vec<Node>,
}

impl Node {
    /// Find the first direct child with the given kind.
    pub fn child(&self, kind: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.kind == kind)
    }

    /// Iterate over direct children of the given kinds.
    pub fn children_of_kind(&self, kinds: &[&str]) -> impl Iterator<Item = &Node> {
        let set: HashSet<String> = kinds.iter().map(|s| (*s).to_string()).collect();
        self.children.iter().filter(move |c| set.contains(&c.kind))
    }

    /// Find the first descendant (depth-first) with the given kind.
    pub fn find_descendant(&self, kind: &str) -> Option<&Node> {
        for child in &self.children {
            if child.kind == kind {
                return Some(child);
            }
            if let Some(found) = child.find_descendant(kind) {
                return Some(found);
            }
        }
        None
    }

    /// True if any descendant has the given kind.
    pub fn has_descendant(&self, kind: &str) -> bool {
        self.find_descendant(kind).is_some()
    }

    /// Collect every descendant of the given kind.
    pub fn collect_descendants<'a>(&'a self, kind: &str, out: &mut Vec<&'a Node>) {
        for child in &self.children {
            if child.kind == kind {
                out.push(child);
            }
            child.collect_descendants(kind, out);
        }
    }
}

/// Parse a bash command string into an AST.
///
/// Returns `None` when tree-sitter fails to parse (e.g. malformed syntax) or
/// when the grammar is unavailable.
pub fn parse(source: &str) -> Option<Node> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(source, None)?;
    Some(convert_node(tree.root_node(), source.as_bytes()))
}

fn convert_node(ts_node: tree_sitter::Node, source: &[u8]) -> Node {
    let start_byte = ts_node.start_byte();
    let end_byte = ts_node.end_byte();
    let text = String::from_utf8_lossy(&source[start_byte..end_byte]).to_string();
    let children: Vec<Node> = (0..ts_node.child_count())
        .filter_map(|i| {
            let child = ts_node.child(i)?;
            Some(convert_node(child, source))
        })
        .collect();

    Node {
        kind: ts_node.kind().to_string(),
        text,
        start_byte,
        end_byte,
        children,
    }
}

/// Named node kinds that represent compound / dynamic shell constructs.
/// If any of these appear in a command, we refuse to extract a static argv.
pub const DANGEROUS_KINDS: &[&str] = &[
    "command_substitution", // $(...)
    "process_substitution", // <(...) >(...)
    "expansion",            // ${...}
    "subshell",             // (...)
    "compound_statement",   // { ...; }
    "for_statement",
    "while_statement",
    "until_statement",
    "if_statement",
    "case_statement",
    "function_definition",
    "test_command",
    "ansi_c_string",
    "translated_string",
    "herestring_redirect",
    "heredoc_redirect",
    "arithmetic_expansion", // $((...))
];

/// Kinds that wrap multiple commands.
pub const STRUCTURAL_KINDS: &[&str] = &["program", "list", "pipeline", "redirected_statement"];

/// A redirect operator canonical enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectOp {
    Output,
    Append,
    Input,
    HereDoc,
    HereString,
    DupOutput,
    DupInput,
    ForceOutput,
    StderrStdout,
    StderrAppend,
}

impl RedirectOp {
    /// Returns true if the redirect can write to a file.
    pub fn is_output(&self) -> bool {
        matches!(
            self,
            RedirectOp::Output
                | RedirectOp::Append
                | RedirectOp::ForceOutput
                | RedirectOp::StderrStdout
                | RedirectOp::StderrAppend
        )
    }
}

/// Kinds that are considered literal words / arguments.
pub const ARGUMENT_KINDS: &[&str] = &["word", "string", "raw_string", "number"];

/// Returns true if the node kind is an argument-like token.
pub fn is_argument_kind(kind: &str) -> bool {
    ARGUMENT_KINDS.contains(&kind)
}

/// Returns true if the node kind is a command name or declaration command.
pub fn is_command_kind(kind: &str) -> bool {
    kind == "command" || kind == "declaration_command"
}

/// Maps a tree-sitter redirect operator kind to its canonical form.
pub fn redirect_op_from_kind(kind: &str) -> Option<crate::bash_security::RedirectOp> {
    use crate::bash_security::RedirectOp;
    match kind {
        ">" => Some(RedirectOp::Output),
        ">>" => Some(RedirectOp::Append),
        "<" => Some(RedirectOp::Input),
        "<<" => Some(RedirectOp::HereDoc),
        "<<<" => Some(RedirectOp::HereString),
        ">&" | ">&-" => Some(RedirectOp::DupOutput),
        "<&" | "<&-" => Some(RedirectOp::DupInput),
        ">|" => Some(RedirectOp::ForceOutput),
        "&>" => Some(RedirectOp::StderrStdout),
        "&>>" => Some(RedirectOp::StderrAppend),
        _ => None,
    }
}

/// Simple field-name to value map for a `variable_assignment` node.
pub fn parse_variable_assignment(node: &Node) -> Option<(String, String)> {
    if node.kind != "variable_assignment" {
        return None;
    }
    let name = node.child("variable_name")?;
    let value = node
        .children
        .iter()
        .find(|c| c.kind == "word")
        .map(|v| v.text.clone())
        .unwrap_or_default();
    Some((name.text.clone(), value))
}

// ─────────────────────── SimpleCommand extraction helpers ───────────────────────

/// A simple command with its argv, leading environment assignments, and redirects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleCommand {
    pub argv: Vec<String>,
    pub env_vars: Vec<(String, String)>,
    pub redirects: Vec<super::redirect::Redirect>,
    pub text: String,
}

/// Collect all simple commands from an AST node.
pub(crate) fn collect_commands(node: &Node) -> Vec<SimpleCommand> {
    let mut out = Vec::new();
    collect_commands_inner(node, Vec::new(), &mut out);
    out
}

fn collect_commands_inner(
    node: &Node,
    pending_redirects: Vec<super::redirect::Redirect>,
    out: &mut Vec<SimpleCommand>,
) {
    if is_command_kind(&node.kind) {
        if let Some(cmd) = extract_simple_command(node, pending_redirects) {
            out.push(cmd);
        }
        return;
    }

    if node.kind == "redirected_statement" {
        let redirects = super::redirect::extract_redirects(node);
        for child in &node.children {
            if is_command_kind(&child.kind) {
                collect_commands_inner(child, redirects.clone(), out);
            }
        }
        return;
    }

    if STRUCTURAL_KINDS.contains(&node.kind.as_str()) || node.kind == "subshell" {
        for child in &node.children {
            collect_commands_inner(child, pending_redirects.clone(), out);
        }
    }
}

fn extract_simple_command(
    node: &Node,
    outer_redirects: Vec<super::redirect::Redirect>,
) -> Option<SimpleCommand> {
    let text = node.text.clone();
    let mut argv = Vec::new();
    let mut env_vars = Vec::new();

    if node.kind == "declaration_command" {
        let first = node.children.first()?;
        argv.push(first.text.clone());
        return Some(SimpleCommand {
            argv,
            env_vars,
            redirects: outer_redirects,
            text,
        });
    }

    let mut found_command_name = false;

    for child in &node.children {
        match child.kind.as_str() {
            "variable_assignment" => {
                if let Some((name, value)) = parse_variable_assignment(child) {
                    env_vars.push((name, value));
                }
            }
            "command_name" => {
                found_command_name = true;
                argv.push(strip_quotes(&child.text));
            }
            kind if is_argument_kind(kind) => {
                if !found_command_name {
                    found_command_name = true;
                    argv.push(strip_quotes(&child.text));
                } else {
                    argv.push(strip_quotes(&child.text));
                }
            }
            _ => {}
        }
    }

    if argv.is_empty() {
        return None;
    }

    Some(SimpleCommand {
        argv,
        env_vars,
        redirects: outer_redirects,
        text,
    })
}

pub(crate) fn strip_quotes(text: &str) -> String {
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

pub(crate) fn has_glob_or_brace_expansion(node: &Node) -> bool {
    if node.kind == "word" {
        return is_glob_pattern(&node.text);
    }
    if node.kind == "concatenation" {
        return is_brace_expansion(&node.text)
            || node.children.iter().any(has_glob_or_brace_expansion);
    }
    node.children.iter().any(has_glob_or_brace_expansion)
}

fn is_glob_pattern(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

fn is_brace_expansion(text: &str) -> bool {
    let mut depth = 0i32;
    let mut has_comma_or_range = false;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                if depth > 0 && has_comma_or_range {
                    return true;
                }
                depth = (depth - 1).max(0);
                has_comma_or_range = false;
            }
            ',' if depth > 0 => has_comma_or_range = true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_command() {
        let ast = parse("echo hello").unwrap();
        assert_eq!(ast.kind, "program");
        assert!(ast.has_descendant("command"));
    }

    #[test]
    fn detects_command_substitution() {
        let ast = parse("echo $(rm -rf /)").unwrap();
        assert!(ast.has_descendant("command_substitution"));
    }

    #[test]
    fn detects_pipeline() {
        let ast = parse("cat file | sh").unwrap();
        assert!(ast.has_descendant("pipeline"));
    }
}
