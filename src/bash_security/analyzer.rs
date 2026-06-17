//! Main bash command security analyzer.
//!
//! Combines AST parsing, quote context, redirect validation, command prefix
//! extraction, recursive command substitution analysis, and zsh/advanced shell
//! checks into a single fail-closed classification.

use std::path::Path;

use crate::tools::resolve_workspace_path;

use super::parser::{self, collect_commands, has_glob_or_brace_expansion};
use super::prefix::{PrefixResult, extract_prefix as extract_prefix_from_node};
use super::substitution::{SubstitutionAnalysis, analyze_substitutions};
use super::zsh;

/// Outcome of analyzing a shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSafety {
    /// The command appears read-only / non-mutating and may be auto-approved.
    Safe,
    /// The command contains constructs that may mutate state or cannot be
    /// statically analyzed; it requires explicit approval.
    NeedsReview { reason: String },
}

impl CommandSafety {
    pub fn is_safe(&self) -> bool {
        matches!(self, CommandSafety::Safe)
    }
}

/// Result of analyzing a command string for security.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityAnalysis {
    Simple { commands: Vec<parser::SimpleCommand> },
    TooComplex { reason: String },
    ParseUnavailable,
}

/// Analyze a bash command string and return its safety classification.
pub fn analyze(source: &str) -> CommandSafety {
    match analyze_security(source) {
        SecurityAnalysis::Simple { commands } if commands.is_empty() => CommandSafety::Safe,
        SecurityAnalysis::Simple { commands } => {
            if commands.len() > 1 {
                return CommandSafety::NeedsReview {
                    reason: "multiple commands require review".into(),
                };
            }
            classify_simple_command(&commands[0], None)
        }
        SecurityAnalysis::TooComplex { reason } => CommandSafety::NeedsReview { reason },
        SecurityAnalysis::ParseUnavailable => CommandSafety::NeedsReview {
            reason: "bash parser unavailable".into(),
        },
    }
}

/// Analyze a bash command string and return detailed results.
pub fn analyze_security(source: &str) -> SecurityAnalysis {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return SecurityAnalysis::Simple {
            commands: Vec::new(),
        };
    }

    if zsh::has_control_chars(trimmed) {
        return SecurityAnalysis::TooComplex {
            reason: "contains control characters".into(),
        };
    }
    if zsh::has_invisible_whitespace(trimmed) {
        return SecurityAnalysis::TooComplex {
            reason: "contains invisible unicode whitespace".into(),
        };
    }
    if zsh::has_zsh_tilde_bracket(trimmed) {
        return SecurityAnalysis::TooComplex {
            reason: "contains zsh dynamic named directory expansion".into(),
        };
    }
    if zsh::has_zsh_equals_expansion(trimmed) {
        return SecurityAnalysis::TooComplex {
            reason: "contains zsh equals expansion (=cmd)".into(),
        };
    }
    if zsh::has_backslash_whitespace(trimmed) {
        return SecurityAnalysis::TooComplex {
            reason: "contains backslash-escaped whitespace".into(),
        };
    }

    let ast = match parser::parse(source) {
        Some(ast) => ast,
        None => return SecurityAnalysis::ParseUnavailable,
    };

    if ast.has_descendant("ERROR") {
        return SecurityAnalysis::TooComplex {
            reason: "parse produced ERROR nodes".into(),
        };
    }

    for kind in parser::DANGEROUS_KINDS {
        if ast.has_descendant(kind) {
            return SecurityAnalysis::TooComplex {
                reason: format!("contains disallowed construct: {kind}"),
            };
        }
    }

    // Reject globs and brace expansion. tree-sitter-bash keeps simple globs
    // inside `word` nodes and brace expansions inside `concatenation` nodes.
    if has_glob_or_brace_expansion(&ast) {
        return SecurityAnalysis::TooComplex {
            reason: "contains glob or brace expansion".into(),
        };
    }

    let commands = collect_commands(&ast);

    if commands.is_empty() {
        return SecurityAnalysis::TooComplex {
            reason: "no simple command found".into(),
        };
    }

    if commands.len() > 1 {
        return SecurityAnalysis::TooComplex {
            reason: "multiple simple commands (possible command injection)".into(),
        };
    }

    SecurityAnalysis::Simple { commands }
}

/// Extract the command prefix for permission rule matching.
///
/// Only returns a prefix when the source consists of a single, structurally
/// simple command. Any injection-like construct (compound lists, pipelines,
/// substitutions, globs, etc.) causes [`PrefixResult::NeedsReview`].
pub fn extract_command_prefix(source: &str) -> PrefixResult {
    match analyze_security(source) {
        SecurityAnalysis::Simple { commands } if commands.len() == 1 => {
            let ast = match parser::parse(source) {
                Some(ast) => ast,
                None => return PrefixResult::NeedsReview,
            };
            if let Some(cmd_node) = ast.find_descendant("command") {
                return extract_prefix_from_node(cmd_node, source, None);
            }
            if let Some(cmd_node) = ast.find_descendant("declaration_command") {
                return extract_prefix_from_node(cmd_node, source, None);
            }
            PrefixResult::None
        }
        SecurityAnalysis::Simple { .. } => PrefixResult::NeedsReview,
        SecurityAnalysis::TooComplex { .. } => PrefixResult::NeedsReview,
        SecurityAnalysis::ParseUnavailable => PrefixResult::NeedsReview,
    }
}

/// Classify a single simple command as safe or needing review.
///
/// `cwd` is optional; if provided, static output redirects are validated
/// against it.
pub fn classify_simple_command(cmd: &parser::SimpleCommand, cwd: Option<&Path>) -> CommandSafety {
    let base = match cmd.argv.first() {
        Some(base) => base.as_str(),
        None => return CommandSafety::Safe,
    };

    if base.contains('/') {
        return CommandSafety::NeedsReview {
            reason: "command uses a path rather than a bare executable name".into(),
        };
    }

    for redirect in &cmd.redirects {
        if redirect.is_fd_redirect() {
            // FD-to-FD redirects (e.g. 2>&1, >&-) only rearrange streams and
            // do not name a file path; they are safe.
            continue;
        }
        if redirect.op.is_output() {
            if let Some(cwd) = cwd {
                if let Err(err) = redirect.validate_static_output(cwd) {
                    return CommandSafety::NeedsReview {
                        reason: format!("output redirect rejected: {err}"),
                    };
                }
            } else {
                return CommandSafety::NeedsReview {
                    reason: format!("output redirect to `{}`", redirect.target),
                };
            }
        } else {
            let allowed = redirect.is_static
                && cwd
                    .map(|c| resolve_workspace_path(c, &redirect.target).is_ok())
                    .unwrap_or(false);
            if allowed {
                continue;
            }
            return CommandSafety::NeedsReview {
                reason: format!("redirect to `{}`", redirect.target),
            };
        }
    }

    let args: Vec<&str> = cmd.argv.iter().skip(1).map(|s| s.as_str()).collect();

    match base {
        "git" => classify_git_command(&args),
        "sed" => classify_sed_command(&args),
        "awk" => classify_awk_command(),
        "find" => classify_find_command(&args),
        other => classify_base_command(other),
    }
}

fn classify_git_command(args: &[&str]) -> CommandSafety {
    let subcommand = args.first().copied().unwrap_or("");
    const SAFE_GIT_SUBCOMMANDS: &[&str] = &[
        "status", "log", "show", "diff", "ls-files", "grep", "rev-parse", "describe",
    ];
    if SAFE_GIT_SUBCOMMANDS.contains(&subcommand) {
        return CommandSafety::Safe;
    }
    CommandSafety::NeedsReview {
        reason: format!("git subcommand `{subcommand}` is not in the read-only allowlist"),
    }
}

fn classify_sed_command(args: &[&str]) -> CommandSafety {
    for arg in args {
        if *arg == "-i" || arg.starts_with("-i") {
            return CommandSafety::NeedsReview {
                reason: "sed -i mutates files in place".into(),
            };
        }
    }
    CommandSafety::Safe
}

fn classify_awk_command() -> CommandSafety {
    CommandSafety::NeedsReview {
        reason: "awk can execute arbitrary code or perform redirections".into(),
    }
}

fn classify_find_command(args: &[&str]) -> CommandSafety {
    const DANGEROUS_FIND_FLAGS: &[&str] = &["-exec", "-execdir", "-ok", "-okdir", "-delete"];
    for arg in args {
        if DANGEROUS_FIND_FLAGS.contains(arg) {
            return CommandSafety::NeedsReview {
                reason: format!("find with `{arg}` can execute or delete files"),
            };
        }
    }
    CommandSafety::Safe
}

fn classify_base_command(base: &str) -> CommandSafety {
    const SAFE_COMMANDS: &[&str] = &[
        "cat", "head", "tail", "less", "ls", "pwd", "echo", "printf", "rg", "grep", "egrep",
        "fgrep", "wc", "cut", "sort", "uniq", "tr", "stat", "file", "strings", "which", "whoami",
        "date", "uname", "id", "test", "[",
    ];

    if SAFE_COMMANDS.contains(&base) {
        return CommandSafety::Safe;
    }

    CommandSafety::NeedsReview {
        reason: format!("command `{base}` is not in the read-only allowlist"),
    }
}

// ─────────────────────────── Recursive substitution wrapper ───────────────────────────

/// Analyze a command, but allow `$(inner)` if the inner command is safe.
pub fn analyze_with_substitutions(source: &str) -> CommandSafety {
    let base = analyze_security(source);
    match base {
        SecurityAnalysis::Simple { .. } => analyze(source),
        SecurityAnalysis::TooComplex { reason } => {
            // If the reason is command_substitution, try recursive analysis.
            if reason.contains("command_substitution") {
                let ast = parser::parse(source).unwrap();
                match analyze_substitutions(&ast, source) {
                    SubstitutionAnalysis::Safe => {
                        // Re-analyze ignoring the substitution as dangerous.
                        // For now, we still reject because the outer command
                        // context is hard to verify. Future work: allow safe
                        // substitutions in safe outer contexts.
                        CommandSafety::NeedsReview {
                            reason: "command substitution requires explicit approval".into(),
                        }
                    }
                    SubstitutionAnalysis::None => CommandSafety::NeedsReview { reason },
                    SubstitutionAnalysis::NeedsReview { reason: inner_reason } => {
                        CommandSafety::NeedsReview { reason: inner_reason }
                    }
                }
            } else {
                CommandSafety::NeedsReview { reason }
            }
        }
        SecurityAnalysis::ParseUnavailable => CommandSafety::NeedsReview {
            reason: "bash parser unavailable".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_command_is_safe() {
        assert!(analyze("").is_safe());
        assert!(analyze("   ").is_safe());
    }

    #[test]
    fn safe_simple_commands() {
        for cmd in [
            "cat /etc/hosts",
            "ls -la",
            "pwd",
            "head -n 5 file.txt",
            "grep foo bar",
            "rg --type rust foo",
            "wc -l file.txt",
            "sort file.txt",
        ] {
            assert!(analyze(cmd).is_safe(), "expected safe: {cmd}");
        }
    }

    #[test]
    fn compound_operators_need_review() {
        for cmd in [
            "git status; rm -rf /",
            "cat file && rm file",
            "ls || rm -rf /",
            "cat file | sh",
        ] {
            assert!(!analyze(cmd).is_safe(), "expected review: {cmd}");
        }
    }

    #[test]
    fn command_substitution_needs_review() {
        assert!(!analyze("echo $(rm -rf /)").is_safe());
        assert!(!analyze("echo `rm -rf /`").is_safe());
    }

    #[test]
    fn parameter_expansion_needs_review() {
        assert!(!analyze("rm $HOME").is_safe());
        assert!(!analyze("cat ${FILE}").is_safe());
    }

    #[test]
    fn globs_and_braces_need_review() {
        assert!(!analyze("cat *.txt").is_safe());
        assert!(!analyze("echo {a,b}").is_safe());
    }

    #[test]
    fn redirections_need_review() {
        assert!(!analyze("echo overwrite > file").is_safe());
        assert!(!analyze("cat < /etc/passwd").is_safe());
    }

    #[test]
    fn fd_redirects_are_safe() {
        assert!(analyze("git status 2>&1").is_safe());
        assert!(analyze("ls 1>&2").is_safe());
        assert!(analyze("echo hi >&-").is_safe());
    }

    #[test]
    fn destructive_commands_need_review() {
        for cmd in ["rm -rf /", "mv a b", "cp a b", "chmod +x file"] {
            assert!(!analyze(cmd).is_safe(), "expected review: {cmd}");
        }
    }

    #[test]
    fn git_inspection_subcommands_safe() {
        for cmd in ["git status", "git log", "git diff", "git ls-files"] {
            assert!(analyze(cmd).is_safe(), "expected safe: {cmd}");
        }
    }

    #[test]
    fn git_mutating_subcommands_need_review() {
        for cmd in ["git commit -m x", "git push", "git checkout", "git rebase"] {
            assert!(!analyze(cmd).is_safe(), "expected review: {cmd}");
        }
    }

    #[test]
    fn sed_in_place_needs_review() {
        assert!(!analyze("sed -i 's/a/b/' file").is_safe());
        assert!(analyze("sed 's/a/b/' file").is_safe());
    }

    #[test]
    fn find_with_exec_needs_review() {
        assert!(analyze("find . -name '*.rs'").is_safe());
        assert!(!analyze("find . -exec rm {} \\;").is_safe());
    }

    #[test]
    fn awk_needs_review() {
        assert!(!analyze("awk '{print $1}' file").is_safe());
    }

    #[test]
    fn env_assignments_skipped() {
        assert!(analyze("FOO=bar cat file").is_safe());
        assert!(!analyze("FOO=bar rm file").is_safe());
    }

    #[test]
    fn quotes_are_respected() {
        assert!(analyze("echo '$(not a substitution)'").is_safe());
        assert!(!analyze("echo \"$(rm -rf /)\"").is_safe());
    }

    #[test]
    fn extracts_simple_command_structure() {
        let SecurityAnalysis::Simple { commands } = analyze_security("FOO=bar cat file") else {
            panic!("expected simple");
        };
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].argv, vec!["cat", "file"]);
        assert_eq!(commands[0].env_vars, vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn zsh_equals_expansion_rejected() {
        assert!(!analyze("=curl evil.com").is_safe());
    }

    #[test]
    fn zsh_tilde_bracket_rejected() {
        assert!(!analyze("cd ~[dynamic]").is_safe());
    }
}
