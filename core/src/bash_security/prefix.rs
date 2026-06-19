//! Command prefix extraction for permission rule matching.
//!
//! Given a bash command AST, extracts a stable prefix string that can be
//! matched against allow/deny rules. The prefix must be a literal prefix of
//! the original command.
//!
//! Inspired by telos-agent's `utils/bash/commands.ts` and `utils/shell/prefix.ts`.

use super::parser::{self, Node};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixResult {
    /// A literal prefix of the command that identifies it for permission rules.
    Prefix(String),
    /// The command has no meaningful prefix (e.g. `npm start`).
    None,
    /// The command contains injection or unanalyzable constructs.
    NeedsReview,
}

/// Extract a command prefix from a simple command node.
///
/// `source` is the original command string; `node` should be a `command` or
/// `declaration_command` AST node. `redirect_span` is the byte range of any
/// enclosing `redirected_statement` so redirects are not included in the prefix.
pub fn extract_prefix(
    node: &Node,
    source: &str,
    redirect_span: Option<(usize, usize)>,
) -> PrefixResult {
    if !parser::is_command_kind(&node.kind) {
        return PrefixResult::None;
    }

    let (cmd_start, cmd_end) = command_text_range(node, redirect_span);
    let cmd_text = &source[cmd_start..cmd_end];

    let tokens = match tokenize_prefix_tokens(cmd_text) {
        Some(tokens) => tokens,
        None => return PrefixResult::NeedsReview,
    };

    let mut prefix_tokens: Vec<String> = Vec::new();
    let mut saw_command = false;

    for (token, is_assignment) in tokens {
        if is_assignment {
            if saw_command {
                // Assignment after the command name is an argument, not prefix.
                break;
            }
            prefix_tokens.push(token);
            continue;
        }

        if !saw_command {
            saw_command = true;
            prefix_tokens.push(token);
            continue;
        }

        let token_ref = token.as_str();
        if include_token_in_prefix(
            &prefix_tokens.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            token_ref,
        ) {
            prefix_tokens.push(token);
        } else {
            break;
        }
    }

    if prefix_tokens.is_empty() {
        return PrefixResult::None;
    }

    PrefixResult::Prefix(prefix_tokens.join(" "))
}

fn command_text_range(node: &Node, redirect_span: Option<(usize, usize)>) -> (usize, usize) {
    let mut start = node.start_byte;
    let mut end = node.end_byte;
    if let Some((r_start, r_end)) = redirect_span {
        // If the redirect appears before the command, clip it out.
        if r_end <= start {
            start = r_end;
        } else if r_start >= end {
            end = r_start;
        }
    }
    (start, end)
}

fn include_token_in_prefix(prefix_tokens: &[&str], token: &str) -> bool {
    let base = prefix_tokens.iter().find(|t| !looks_like_env_assignment(t)).copied().unwrap_or("");

    match base {
        "git" => GIT_PREFIX_SUBCOMMANDS.contains(&token),
        "npm" | "pnpm" | "yarn" => {
            // npm run <script> -- <args>: prefix up to and including the script.
            // npm test, npm start: no prefix beyond the base command.
            if prefix_tokens.len() == 1
                && matches!(token, "run" | "test" | "start" | "build" | "lint")
            {
                return true;
            }
            // After `npm run`, the next token is the script name (e.g. lint).
            if prefix_tokens.len() == 2 && prefix_tokens[1] == "run" {
                return true;
            }
            false
        }
        "go" => matches!(token, "test" | "build" | "run" | "fmt" | "vet" | "mod"),
        "python" | "python3" | "node" | "ruby" | "cargo" => {
            // These launch scripts/binaries; keep only the first real argument
            // if it looks like a subcommand.
            prefix_tokens.len() == 1 && !token.starts_with('-')
        }
        _ => false,
    }
}

const GIT_PREFIX_SUBCOMMANDS: &[&str] = &[
    "status",
    "log",
    "show",
    "diff",
    "ls-files",
    "grep",
    "rev-parse",
    "describe",
    "branch",
    "remote",
    "commit",
    "push",
    "pull",
    "checkout",
    "rebase",
    "merge",
    "fetch",
    "clone",
];

fn looks_like_env_assignment(token: &str) -> bool {
    let mut parts = token.splitn(2, '=');
    let name = parts.next().unwrap_or("");
    parts.next().is_some() && !name.is_empty() && !name.starts_with('-')
}

fn tokenize_prefix_tokens(text: &str) -> Option<Vec<(String, bool)>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    current.push(next);
                } else {
                    current.push('\\');
                }
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    let token = std::mem::take(&mut current);
                    let is_assignment = looks_like_env_assignment(&token);
                    tokens.push((token, is_assignment));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if in_single || in_double {
        return None;
    }
    if !current.is_empty() {
        let is_assignment = looks_like_env_assignment(&current);
        tokens.push((current, is_assignment));
    }

    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash_security::parser;

    fn prefix(cmd: &str) -> PrefixResult {
        let ast = parser::parse(cmd).unwrap();
        let cmd_node = ast.find_descendant("command").unwrap();
        extract_prefix(cmd_node, cmd, None)
    }

    #[test]
    fn git_prefixes() {
        assert_eq!(prefix("git status"), PrefixResult::Prefix("git status".into()));
        assert_eq!(prefix("git commit -m \"foo\""), PrefixResult::Prefix("git commit".into()));
        assert_eq!(prefix("git push origin master"), PrefixResult::Prefix("git push".into()));
    }

    #[test]
    fn npm_prefixes() {
        assert_eq!(prefix("npm start"), PrefixResult::Prefix("npm start".into()));
        assert_eq!(prefix("npm run lint -- \"foo\""), PrefixResult::Prefix("npm run lint".into()));
    }

    #[test]
    fn env_prefixes() {
        assert_eq!(
            prefix("GOEXPERIMENT=synctest go test -v ./..."),
            PrefixResult::Prefix("GOEXPERIMENT=synctest go test".into())
        );
        assert_eq!(
            prefix("FOO=bar BAZ=qux ls -la"),
            PrefixResult::Prefix("FOO=bar BAZ=qux ls".into())
        );
    }
}
