use crate::powershell_security::aliases::canonical_command_name;
use crate::powershell_security::parser::ParsedCommandElement;

pub fn is_read_only_command(cmd: &ParsedCommandElement) -> bool {
    matches!(
        canonical_command_name(&cmd.name).as_str(),
        "Get-ChildItem"
            | "Get-Content"
            | "Get-Item"
            | "Get-Location"
            | "Get-Process"
            | "Get-Service"
            | "Get-FileHash"
            | "Select-String"
            | "Test-Path"
            | "Resolve-Path"
            | "Write-Output"
    )
}
