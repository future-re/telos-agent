//! PowerShell command parsing, prefix extraction, and safety analysis.

pub mod aliases;
pub mod analyzer;
pub mod dangerous_cmdlets;
pub mod parser;
pub mod path_validation;
pub mod read_only;
pub mod static_prefix;

pub use analyzer::{CommandSafety, analyze};
pub use static_prefix::{PrefixResult, extract_command_prefix};
