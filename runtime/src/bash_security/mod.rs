//! Bash command security analysis — AST-based, fail-closed.
//!
//! This crate module parses bash commands with `tree-sitter-bash` and extracts
//! a static argv for each simple command. Any construct that cannot be reasoned
//! about statically is classified as needing explicit human approval.
//!
//! Submodules:
//! - [`parser`](crate::bash_security::parser) — tree-sitter-bash wrapper and AST helpers
//! - [`quote_context`](crate::bash_security::quote_context) — quote-aware text views
//! - [`redirect`](crate::bash_security::redirect) — redirect extraction and static target validation
//! - [`prefix`](crate::bash_security::prefix) — command prefix extraction for permission rules
//! - command substitution analysis — recursive handling for nested shell substitutions
//! - [`zsh`](crate::bash_security::zsh) — zsh and advanced shell expansion checks
//! - [`analyzer`](crate::bash_security::analyzer) — main security analyzer combining all of the above

pub mod analyzer;
pub mod parser;
pub mod prefix;
pub mod quote_context;
pub mod redirect;
#[cfg(test)]
mod substitution;
pub mod zsh;

pub use analyzer::{
    CommandSafety, SecurityAnalysis, analyze, analyze_security, classify_simple_command,
    extract_command_prefix,
};
pub use parser::{RedirectOp, SimpleCommand};
pub use prefix::PrefixResult;
pub use redirect::Redirect;
