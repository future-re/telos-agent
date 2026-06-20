# PowerShell Tool Migration Design

## Goal

Port the behavior of `learn-claude-code`'s `PowerShellTool` into this Rust codebase by rebuilding the feature around the existing Telos architecture. The migration adds a first-class `PowerShell` tool without changing the existing `Bash` tool's behavior.

## Reference

The behavioral reference is `/home/alin/codework/learn-claude-code`, especially:

- `tools/PowerShellTool/PowerShellTool.tsx`
- `tools/PowerShellTool/powershellPermissions.ts`
- `tools/PowerShellTool/powershellSecurity.ts`
- `tools/PowerShellTool/readOnlyValidation.ts`
- `tools/PowerShellTool/pathValidation.ts`
- `utils/powershell/parser.ts`
- `utils/powershell/staticPrefix.ts`
- `utils/shell/powershellDetection.ts`
- `utils/shell/powershellProvider.ts`

The migration follows those semantics where they matter for execution, permission safety, command classification, and prompt guidance, but it does not translate TypeScript modules mechanically.

## Scope

This spec covers a behavior migration with Rust-native module boundaries:

- Add a `PowerShell` built-in tool.
- Add PowerShell executable discovery.
- Execute PowerShell commands through the same controlled process model used by the current shell tool.
- Parse PowerShell with a Rust tree-sitter grammar.
- Add PowerShell-specific safety analysis, static prefix extraction, read-only classification, and path validation.
- Split `Bash(...)` and `PowerShell(...)` permission handling.
- Add configuration for the default shell while keeping `bash` as the default.

This spec does not add UI-specific features from Claude Code such as React renderers, analytics events, task backgrounding UI, or persisted large-output storage beyond the existing Telos tool result behavior.

## Dependency Choice

Use `tree-sitter-pwsh = "0.38.1"` for PowerShell parsing.

Rationale:

- The project already uses `tree-sitter-bash` for Bash safety analysis, so a tree-sitter PowerShell grammar fits the existing pattern.
- `tree-sitter-pwsh` is newer than `tree-sitter-powershell` and targets modern PowerShell grammar.
- Parser behavior remains local and inspectable through a Rust AST wrapper.

Do not use `powershell` or `powershell_script` crates for execution.

Rationale:

- The current `ShellTool` owns process execution, timeout handling, environment clearing, cwd, stdout/stderr capture, and process cleanup.
- The PowerShell tool should preserve the same security boundary.
- A third-party execution wrapper would hide details that are important for approvals and sandbox-like behavior.

## Architecture

Add these modules:

```text
core/src/tools/powershell.rs
core/src/powershell_security/mod.rs
core/src/powershell_security/parser.rs
core/src/powershell_security/static_prefix.rs
core/src/powershell_security/analyzer.rs
core/src/powershell_security/read_only.rs
core/src/powershell_security/path_validation.rs
core/src/powershell_security/aliases.rs
core/src/powershell_security/dangerous_cmdlets.rs
```

`core/src/tools/powershell.rs` owns the tool interface and process execution.

`core/src/powershell_security/parser.rs` wraps `tree-sitter-pwsh` and exposes a small Rust AST model. Other modules must depend on this wrapper rather than raw tree-sitter node traversal.

`static_prefix.rs` extracts permission prefixes from parsed commands. Prefix extraction is case-insensitive and normalizes common aliases where doing so is fail-safe.

`analyzer.rs` combines parser, dangerous command detection, read-only classification, and path validation into a single safety decision used by `PowerShellTool::check_permission`.

`read_only.rs` classifies provably read-only commands and mirrors the high-value parts of Claude Code's `readOnlyValidation.ts`.

`path_validation.rs` extracts local filesystem path arguments for PowerShell cmdlets and validates them against the current working directory and permission context.

`aliases.rs` owns canonicalization such as `rm -> Remove-Item`, `cat -> Get-Content`, `ls -> Get-ChildItem`, and `pwd -> Get-Location`.

`dangerous_cmdlets.rs` owns static deny/ask lists for execution, persistence, privilege escalation, module loading, file execution, and download-and-execute patterns.

## Tool Behavior

The new tool definition is:

```text
name: PowerShell
input:
  command: string
  description?: string
  timeout_ms?: integer
```

The tool should be registered in `register_core_tools` alongside `Bash`.

The prompt text should tell the model:

- Use `PowerShell` for Windows-native shell commands.
- Prefer file tools for file reads and edits when possible.
- Use PowerShell syntax, not Bash syntax.
- Prefer `pwsh` semantics when available, but avoid relying on PowerShell 7-only operators unless the detected executable is `pwsh`.

## Executable Discovery

PowerShell discovery should be explicit and testable:

1. Honor `TELOS_POWERSHELL_PATH` if set.
2. On Windows, search `pwsh.exe`, then `powershell.exe`.
3. On non-Windows, search `pwsh`, then `powershell`.
4. If no executable is found, return a tool execution error explaining that PowerShell is unavailable.

The detected executable should expose an edition:

- `pwsh` / `pwsh.exe` -> `PowerShellEdition::Core`
- `powershell` / `powershell.exe` -> `PowerShellEdition::Desktop`

The edition is used for prompt guidance and validation differences.

## Execution

Execution uses `tokio::process::Command`, not a PowerShell wrapper crate.

Default invocation:

```text
<powershell> -NoProfile -NonInteractive -Command <command>
```

The implementation must:

- Set `current_dir` to `context.cwd`.
- Call `env_clear()`.
- Inject only `context.env`.
- Use the tool's `timeout_ms`, defaulting to `120000`.
- Capture stdout and stderr separately.
- Return JSON shaped consistently with `ShellTool`:
  - `status`
  - `success`
  - `stdout`
  - `stderr`
- Trim large output using the existing truncation policy.
- Use `kill_on_drop(true)`.

Add a helper that encodes PowerShell command strings as UTF-16LE base64 for `-EncodedCommand`. The initial execution path uses `-Command`; the encoding helper is still tested so a future switch to encoded execution is low-risk.

## Permission Model

`Bash` and `PowerShell` permissions are separate. A rule for `Bash` must not match `PowerShell`, and a rule for `PowerShell` must not match `Bash`.

`PermissionEngine::evaluate_shell_call` currently assumes Bash prefix extraction. It should be refactored to route through a shell kind:

```rust
pub enum ShellKind {
    Bash,
    PowerShell,
}
```

PowerShell prefix matching must be case-insensitive. For allow rules, normalization must not broaden a rule unsafely. Deny and ask rules may normalize more aggressively because fail-closed overmatching is acceptable.

Examples:

- `PowerShell(Get-Process:*)` can allow `Get-Process -Name pwsh`.
- `PowerShell(get-process:*)` can match `Get-Process -Name pwsh`.
- `PowerShell(Remove-Item:*)` can deny `rm ./file.txt`.
- `Bash(git status:*)` remains Bash-only.

## Security Analysis

The analyzer returns the same shape as Bash safety analysis:

```rust
pub enum CommandSafety {
    Safe,
    NeedsReview { reason: String },
}
```

Parsing failures must fail closed as `NeedsReview`.

The initial PowerShell analyzer must flag at least these cases as `NeedsReview`:

- `Invoke-Expression` and `iex`.
- Dynamic command names, including invocation through expressions.
- Nested `pwsh` / `powershell` commands.
- `-EncodedCommand` or equivalent abbreviated encoded parameters.
- `Invoke-WebRequest` / `Invoke-RestMethod` piped into `Invoke-Expression`.
- `Start-Process -Verb RunAs`.
- `-ExecutionPolicy Bypass`.
- Writing to `$PROFILE`.
- `Register-ScheduledTask`, `New-Service`, registry Run keys, and WMI event subscriptions.
- Defender/AMSI weakening patterns such as `Set-MpPreference -DisableRealtimeMonitoring`.
- `Remove-Item -Recurse -Force` and dangerous removal targets.
- Module import or dot-sourcing patterns that execute unvalidated code.

Commands that are provably read-only may return `Safe`. Everything else should ask.

## Read-Only Classification

Read-only classification should be conservative.

Allowed read-only commands include:

- `Get-ChildItem`
- `Get-Content`
- `Get-Item`
- `Get-Location`
- `Get-Process`
- `Get-Service`
- `Get-FileHash`
- `Select-String`
- `Test-Path`
- `Resolve-Path`
- `Write-Output`

Aliases are normalized before classification:

- `ls`, `dir`, `gci` -> `Get-ChildItem`
- `cat`, `gc`, `type` -> `Get-Content`
- `pwd`, `gl` -> `Get-Location`
- `ps`, `gps` -> `Get-Process`
- `echo`, `write` -> `Write-Output`

The classifier should reject or ask for:

- Assignments.
- Script blocks.
- Function definitions.
- Dynamic invocation.
- Redirections that write to files.
- Unknown parameters for path-sensitive cmdlets.
- Pipelines where any stage is not provably safe.

## Path Validation

Path validation should start smaller than Claude Code's full table but keep the same model:

- Each supported cmdlet declares whether path arguments are read, write, or create operations.
- Known switches do not consume following arguments.
- Known value parameters consume following arguments but are not treated as paths.
- Unknown parameters on path-sensitive cmdlets force `NeedsReview`.
- Wildcards force review unless the operation is read-only and can be safely bounded.
- Non-filesystem providers such as `Registry::` force review unless explicitly supported.

Initial path-sensitive cmdlets:

- `Get-Content`
- `Get-ChildItem`
- `Set-Content`
- `Add-Content`
- `Out-File`
- `Copy-Item`
- `Move-Item`
- `Rename-Item`
- `Remove-Item`
- `New-Item`
- `Clear-Content`
- `Test-Path`
- `Resolve-Path`

Write/create operations outside the workspace should require approval.

## Configuration

Add an optional default shell setting to CLI file config:

```toml
[agent]
default_shell = "bash"
```

Allowed values:

- `bash`
- `powershell`

Default remains `bash` on every platform. Windows must not auto-switch to PowerShell.

The setting is used by any future user-input shell mode routing. It does not rename or remove either tool.

## Testing Strategy

Tests are required before implementation.

Unit tests:

- PowerShell executable detection honors override and fallback order.
- UTF-16LE base64 encoding matches PowerShell `-EncodedCommand` requirements.
- Parser extracts simple command names and arguments.
- Alias canonicalization is case-insensitive.
- Static prefix extraction handles simple commands, aliases, and rejects dynamic commands.
- Analyzer asks for dangerous commands listed in this spec.
- Read-only classifier allows simple read-only cmdlets.
- Read-only classifier asks for assignments, redirections, dynamic invocation, and unknown commands.
- Path validation distinguishes read and write operations.
- Permission engine keeps `Bash` and `PowerShell` rules separate.
- PowerShell permission prefix matching is case-insensitive.

Integration tests:

- `PowerShell` tool is registered.
- When `pwsh` is available, a simple command executes and returns stdout.
- Environment variables not present in `context.env` are not leaked.
- Timeout returns a tool execution error.
- Existing `Bash` tests continue to pass.

Tests that require an installed PowerShell binary should skip when no executable is available.

## Migration Order

1. Add dependencies and parser wrapper.
2. Add aliases and static prefix extraction.
3. Refactor permission engine for shell kind routing.
4. Add analyzer and initial read-only/security classification.
5. Add executable discovery and `PowerShellTool`.
6. Register the tool.
7. Add config type for `default_shell`.
8. Expand path validation.
9. Run full core and CLI tests.

## Risks

PowerShell parsing is more complex than Bash parsing because aliases, providers, parameter abbreviations, and dynamic invocation are common. The migration must stay conservative: unknown syntax asks for approval.

The reference implementation has many Windows-specific details. This Rust implementation should not claim parity until tests cover the specific behavior.

PowerShell availability varies across Linux/macOS/Windows. Execution tests must handle missing `pwsh` gracefully.

## Acceptance Criteria

- `PowerShell` is a built-in tool.
- Existing `Bash` behavior and tests remain unchanged.
- PowerShell commands execute with controlled environment, cwd, timeout, stdout/stderr capture, and cleanup.
- PowerShell security analysis is independent from Bash analysis.
- PowerShell permission rules are separated from Bash rules.
- PowerShell prefix rules are case-insensitive and alias-aware where safe.
- Dangerous PowerShell commands require approval.
- Provably read-only PowerShell commands can be auto-allowed.
- The design is implemented with Rust-native modules and `tree-sitter-pwsh`, not by embedding the TypeScript implementation.
