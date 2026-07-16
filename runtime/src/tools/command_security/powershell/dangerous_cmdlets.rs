pub const DANGEROUS_COMMANDS: &[&str] =
    &["Invoke-Expression", "Register-ScheduledTask", "New-Service", "Set-MpPreference"];

pub const POWERSHELL_EXECUTABLES: &[&str] = &["pwsh", "pwsh.exe", "powershell", "powershell.exe"];
