use crate::powershell_security::aliases::canonical_command_name;
use crate::powershell_security::parser;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixResult {
    Prefix(String),
    None,
    NeedsReview,
}

pub fn extract_command_prefix(command: &str) -> PrefixResult {
    let parsed = match parser::parse(command) {
        Ok(parsed) => parsed,
        Err(_) => return PrefixResult::NeedsReview,
    };
    let commands = parsed.commands();
    if commands.is_empty() {
        return PrefixResult::None;
    }
    if commands.iter().any(|cmd| cmd.dynamic) {
        return PrefixResult::NeedsReview;
    }
    if commands.len() != 1 {
        return PrefixResult::NeedsReview;
    }
    let name = commands[0].name.trim();
    if name.is_empty() {
        PrefixResult::None
    } else {
        PrefixResult::Prefix(canonical_command_name(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_prefix() {
        assert_eq!(
            extract_command_prefix("Get-Process -Name pwsh"),
            PrefixResult::Prefix("Get-Process".into())
        );
    }

    #[test]
    fn extracts_alias_as_canonical_prefix() {
        assert_eq!(
            extract_command_prefix("rm ./file.txt"),
            PrefixResult::Prefix("Remove-Item".into())
        );
    }

    #[test]
    fn dynamic_command_needs_review() {
        assert_eq!(extract_command_prefix("& ('i' + 'ex') payload"), PrefixResult::NeedsReview);
    }
}
