//! PowerShell command parsing, prefix extraction, and safety analysis.

pub mod aliases;
pub mod parser;
pub mod static_prefix;

pub use static_prefix::{PrefixResult, extract_command_prefix};
