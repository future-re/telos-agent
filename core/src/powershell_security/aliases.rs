pub fn canonical_command_name(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "ls" | "dir" | "gci" => "Get-ChildItem".into(),
        "cat" | "gc" | "type" => "Get-Content".into(),
        "pwd" | "gl" => "Get-Location".into(),
        "ps" | "gps" => "Get-Process".into(),
        "echo" | "write" => "Write-Output".into(),
        "rm" | "del" | "erase" | "ri" => "Remove-Item".into(),
        "cp" | "copy" | "cpi" => "Copy-Item".into(),
        "mv" | "move" | "mi" => "Move-Item".into(),
        other => canonical_case(other),
    }
}

fn canonical_case(lower: &str) -> String {
    match lower {
        "get-process" => "Get-Process".into(),
        "get-content" => "Get-Content".into(),
        "get-childitem" => "Get-ChildItem".into(),
        "get-location" => "Get-Location".into(),
        "remove-item" => "Remove-Item".into(),
        "copy-item" => "Copy-Item".into(),
        "move-item" => "Move-Item".into(),
        "write-output" => "Write-Output".into(),
        "select-string" => "Select-String".into(),
        _ => lower.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_common_aliases_case_insensitively() {
        assert_eq!(canonical_command_name("rm"), "Remove-Item");
        assert_eq!(canonical_command_name("CAT"), "Get-Content");
        assert_eq!(canonical_command_name("pwd"), "Get-Location");
        assert_eq!(canonical_command_name("Get-Process"), "Get-Process");
    }
}
