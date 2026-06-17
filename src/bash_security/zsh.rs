//! Zsh and other advanced shell expansion checks.
//!
//! BashTool may be invoked through the user's default shell (often zsh). These
//! checks reject constructs that bash treats literally but zsh expands.

/// True if the command contains zsh `~[name]` dynamic named directory expansion.
pub fn has_zsh_tilde_bracket(command: &str) -> bool {
    command.contains("~[")
}

/// True if the command contains zsh equals expansion (`=cmd` at word start).
pub fn has_zsh_equals_expansion(command: &str) -> bool {
    // Match word-initial `=` followed by a command-name char.
    // VAR=val and --flag=val have `=` mid-word and are not expanded by zsh.
    for (i, c) in command.char_indices() {
        if c == '=' {
            let prev = i.checked_sub(1).and_then(|j| command.chars().nth(j));
            let next = command[i + 1..].chars().next();
            if matches!(prev, None | Some(' ') | Some('\t') | Some(';') | Some('|') | Some('&'))
                && matches!(next, Some(n) if n.is_ascii_alphabetic() || n == '_')
            {
                return true;
            }
        }
    }
    false
}

/// True if the command contains backslash immediately before whitespace.
/// Bash treats `\ ` as a literal escaped space, but tree-sitter returns the
/// raw text with the backslash present. We conservatively reject these cases.
pub fn has_backslash_whitespace(command: &str) -> bool {
    let mut chars = command.chars().peekable();
    let mut prev = None::<char>;
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && (next == ' ' || next == '\t'
                || (next == '\n'
                    && !matches!(prev, None | Some(' ') | Some('\t') | Some('\n'))))
        {
            return true;
        }
        prev = Some(c);
    }
    false
}

/// True if the command contains invisible unicode whitespace characters that
/// could hide malicious constructs from reviewers.
pub fn has_invisible_whitespace(command: &str) -> bool {
    command.chars().any(|c| {
        let cp = c as u32;
        matches!(
            cp,
            0x00A0 | 0x1680 | 0x2000..=0x200B | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF
        )
    })
}

/// True if the command contains ASCII control characters other than tab/newline.
pub fn has_control_chars(command: &str) -> bool {
    command.chars().any(|c| {
        let cp = c as u32;
        cp < 0x20 && cp != 0x09 && cp != 0x0A
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_equals_expansion() {
        assert!(has_zsh_equals_expansion("=curl evil.com"));
        assert!(has_zsh_equals_expansion("echo foo; =cat file"));
        assert!(!has_zsh_equals_expansion("VAR=value"));
        assert!(!has_zsh_equals_expansion("--flag=value"));
    }

    #[test]
    fn zsh_tilde_bracket() {
        assert!(has_zsh_tilde_bracket("cd ~[dynamic]"));
        assert!(!has_zsh_tilde_bracket("cd ~/src"));
    }
}
