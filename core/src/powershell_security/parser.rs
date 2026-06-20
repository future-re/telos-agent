//! PowerShell parser wrapper around `tree-sitter-pwsh`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPowerShellCommand {
    commands: Vec<ParsedCommandElement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommandElement {
    pub name: String,
    pub args: Vec<String>,
    pub dynamic: bool,
}

impl ParsedPowerShellCommand {
    pub fn commands(&self) -> &[ParsedCommandElement] {
        &self.commands
    }
}

pub fn parse(command: &str) -> Result<ParsedPowerShellCommand, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(ParsedPowerShellCommand { commands: Vec::new() });
    }
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_pwsh::LANGUAGE.into())
        .map_err(|err| format!("failed to load PowerShell grammar: {err}"))?;
    let tree = parser
        .parse(trimmed, None)
        .ok_or_else(|| "PowerShell parser returned no tree".to_string())?;
    if tree.root_node().has_error() {
        return Err("PowerShell parse error".into());
    }
    Ok(ParsedPowerShellCommand { commands: split_commands(trimmed) })
}

fn split_commands(command: &str) -> Vec<ParsedCommandElement> {
    command
        .split([';', '\n'])
        .flat_map(|part| part.split('|'))
        .filter_map(parse_command_part)
        .collect()
}

fn parse_command_part(part: &str) -> Option<ParsedCommandElement> {
    let part = part.trim();
    if part.is_empty() {
        return None;
    }
    let dynamic = part.starts_with('&')
        && !part
            .trim_start_matches('&')
            .trim_start()
            .chars()
            .next()
            .map(|ch| ch.is_ascii_alphabetic() || ch == '_' || ch == '.')
            .unwrap_or(false);
    let part = part.trim_start_matches('&').trim_start();
    let tokens = tokenize_words(part);
    let name = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect();
    Some(ParsedCommandElement { name, args, dynamic })
}

fn tokenize_words(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in input.chars() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_command_name_and_args() {
        let parsed = parse("Get-Process -Name pwsh").expect("parse should succeed");
        let commands = parsed.commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "Get-Process");
        assert_eq!(commands[0].args, vec!["-Name", "pwsh"]);
    }

    #[test]
    fn marks_dynamic_invocation_as_dynamic() {
        let parsed = parse("& ('i' + 'ex') 'payload'").expect("parse should succeed");
        assert!(parsed.commands().iter().any(|cmd| cmd.dynamic));
    }

    #[test]
    fn parse_failure_is_reported() {
        let parsed = parse("Get-Process |");
        assert!(parsed.is_err());
    }
}
