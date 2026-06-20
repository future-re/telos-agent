pub fn has_write_redirection(command: &str) -> bool {
    command.contains('>') && !command.contains("*>$null") && !command.contains("> $null")
}

pub fn has_assignment(command: &str) -> bool {
    let trimmed = command.trim_start();
    trimmed.starts_with('$') && trimmed.contains('=')
}
