# PowerShell Tool Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Rust-native `PowerShell` tool that behaviorally follows `learn-claude-code` while preserving existing `Bash` behavior.

**Architecture:** Add a `powershell_security` module built around `tree-sitter-pwsh`, then route shell permission rules by shell kind. Add `PowerShellTool` as a separate built-in tool with controlled process execution using `tokio::process::Command`.

**Tech Stack:** Rust 2024, Tokio, tree-sitter, `tree-sitter-pwsh`, existing Telos `Tool`, `PermissionEngine`, and CLI config types.

---

## File Structure

- Modify `core/Cargo.toml`: add `tree-sitter-pwsh`.
- Modify `core/src/lib.rs`: export `powershell_security` and re-export `PowerShellTool`.
- Modify `core/src/tools/mod.rs`: register and export `PowerShellTool`.
- Create `core/src/tools/powershell.rs`: tool definition, executable discovery, command encoding, execution, output formatting.
- Create `core/src/powershell_security/mod.rs`: public API and `CommandSafety` re-export shape.
- Create `core/src/powershell_security/parser.rs`: tree-sitter wrapper and reduced AST model.
- Create `core/src/powershell_security/aliases.rs`: alias/canonical command mapping.
- Create `core/src/powershell_security/dangerous_cmdlets.rs`: dangerous command and parameter constants.
- Create `core/src/powershell_security/static_prefix.rs`: prefix extraction for permission rules.
- Create `core/src/powershell_security/read_only.rs`: conservative read-only classifier.
- Create `core/src/powershell_security/path_validation.rs`: path-sensitive cmdlet argument validation.
- Create `core/src/powershell_security/analyzer.rs`: combined safety analyzer.
- Modify `core/src/permissions.rs`: add `ShellKind` and shell-specific prefix extraction.
- Modify `core/src/executor/invoke.rs`: detect both `Bash` and `PowerShell` shell tools.
- Modify `core/tests/tool_permission_tests.rs`: add rule separation and PowerShell prefix tests.
- Create `core/tests/tool_powershell_tests.rs`: registration, safety, execution, timeout, env isolation.
- Modify `cli/src/config/types.rs`: add `DefaultShell` and `agent.default_shell`.
- Modify `cli/src/config/merge.rs`: merge `default_shell`.
- Modify `cli/src/config/mod.rs` or `cli/tests/cli_tests.rs`: add config parsing/merge tests where existing config tests live.

---

### Task 1: Add PowerShell Parser Dependency And Module Skeleton

**Files:**
- Modify: `core/Cargo.toml`
- Modify: `core/src/lib.rs`
- Create: `core/src/powershell_security/mod.rs`
- Create: `core/src/powershell_security/parser.rs`

- [ ] **Step 1: Write failing parser tests**

Add this test module to new file `core/src/powershell_security/parser.rs`:

```rust
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
```

- [ ] **Step 2: Run parser test to verify it fails**

Run:

```bash
cargo test -p telos_agent powershell_security::parser --lib
```

Expected: FAIL because `powershell_security` and `parse` do not exist.

- [ ] **Step 3: Add dependency and minimal parser implementation**

Add to `core/Cargo.toml`:

```toml
tree-sitter-pwsh = "0.38.1"
```

Add to `core/src/lib.rs` near `bash_security`:

```rust
/// PowerShell command safety analysis used by PowerShell permissions.
pub mod powershell_security;
```

Create `core/src/powershell_security/mod.rs`:

```rust
//! PowerShell command parsing, prefix extraction, and safety analysis.

pub mod parser;
```

Create `core/src/powershell_security/parser.rs` with:

```rust
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
```

- [ ] **Step 4: Run parser test to verify it passes**

Run:

```bash
cargo test -p telos_agent powershell_security::parser --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/Cargo.toml core/src/lib.rs core/src/powershell_security/mod.rs core/src/powershell_security/parser.rs
git commit -m "feat: add powershell parser skeleton"
```

---

### Task 2: Add Alias Canonicalization And Static Prefix Extraction

**Files:**
- Create: `core/src/powershell_security/aliases.rs`
- Create: `core/src/powershell_security/static_prefix.rs`
- Modify: `core/src/powershell_security/mod.rs`

- [ ] **Step 1: Write failing alias and prefix tests**

Create `core/src/powershell_security/aliases.rs` with tests first:

```rust
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
```

Create `core/src/powershell_security/static_prefix.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_prefix() {
        assert_eq!(extract_command_prefix("Get-Process -Name pwsh"), PrefixResult::Prefix("Get-Process".into()));
    }

    #[test]
    fn extracts_alias_as_canonical_prefix() {
        assert_eq!(extract_command_prefix("rm ./file.txt"), PrefixResult::Prefix("Remove-Item".into()));
    }

    #[test]
    fn dynamic_command_needs_review() {
        assert_eq!(extract_command_prefix("& ('i' + 'ex') payload"), PrefixResult::NeedsReview);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p telos_agent powershell_security --lib
```

Expected: FAIL because functions and modules are missing.

- [ ] **Step 3: Implement alias and prefix modules**

Update `core/src/powershell_security/mod.rs`:

```rust
pub mod aliases;
pub mod parser;
pub mod static_prefix;

pub use static_prefix::{PrefixResult, extract_command_prefix};
```

Implement `core/src/powershell_security/aliases.rs`:

```rust
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
        _ => lower.to_string(),
    }
}
```

Implement `core/src/powershell_security/static_prefix.rs`:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p telos_agent powershell_security --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/src/powershell_security/mod.rs core/src/powershell_security/aliases.rs core/src/powershell_security/static_prefix.rs
git commit -m "feat: extract powershell command prefixes"
```

---

### Task 3: Refactor Permission Engine For Shell Kind Routing

**Files:**
- Modify: `core/src/permissions.rs`
- Modify: `core/src/executor/invoke.rs`
- Modify: `core/tests/tool_permission_tests.rs`

- [ ] **Step 1: Write failing permission tests**

Add to `core/src/permissions.rs` tests:

```rust
#[test]
fn powershell_shell_call_matches_prefix_case_insensitively() {
    let mut engine = PermissionEngine::new();
    engine.add_rule(PermissionRule::allow_tool("PowerShell").command_prefix("get-process"));
    assert_eq!(
        engine.evaluate_shell_call(
            ShellKind::PowerShell,
            &["PowerShell"],
            "Get-Process -Name pwsh",
            &json!({"command": "Get-Process -Name pwsh"}),
            std::path::Path::new(".")
        ),
        Some(RuleDecision::Allow)
    );
}

#[test]
fn powershell_alias_matches_canonical_deny_rule() {
    let mut engine = PermissionEngine::new();
    engine.add_rule(PermissionRule::deny_tool("PowerShell").command_prefix("Remove-Item"));
    assert_eq!(
        engine.evaluate_shell_call(
            ShellKind::PowerShell,
            &["PowerShell"],
            "rm ./file.txt",
            &json!({"command": "rm ./file.txt"}),
            std::path::Path::new(".")
        ),
        Some(RuleDecision::Deny)
    );
}

#[test]
fn bash_and_powershell_rules_are_separate() {
    let mut engine = PermissionEngine::new();
    engine.add_rule(PermissionRule::allow_tool("Bash").command_prefix("Get-Process"));
    assert_eq!(
        engine.evaluate_shell_call(
            ShellKind::PowerShell,
            &["PowerShell"],
            "Get-Process",
            &json!({"command": "Get-Process"}),
            std::path::Path::new(".")
        ),
        None
    );
}
```

Add to `core/tests/tool_permission_tests.rs`:

```rust
#[test]
fn permission_engine_allows_powershell_by_command_prefix() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("PowerShell").command_prefix("Get-Process"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "PowerShell".into(),
                        arguments: json!({ "command": "Get-Process -Name pwsh" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        register_core_tools(&mut tools);
        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "powershell").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(!tool_result.text().contains("permission_required"), "{}", tool_result.text());
    });
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p telos_agent permissions::tests::powershell_shell_call_matches_prefix_case_insensitively --lib
cargo test -p telos_agent permission_engine_allows_powershell_by_command_prefix
```

Expected: FAIL because `ShellKind` and `PowerShellTool` routing do not exist.

- [ ] **Step 3: Implement shell kind routing**

In `core/src/permissions.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    PowerShell,
}
```

Change `evaluate_shell_call` signature to:

```rust
pub fn evaluate_shell_call(
    &self,
    shell_kind: ShellKind,
    tool_names: &[&str],
    command: &str,
    _arguments: &serde_json::Value,
    cwd: &std::path::Path,
) -> Option<RuleDecision>
```

Inside it, replace Bash-only extraction with:

```rust
let extracted = match shell_kind {
    ShellKind::Bash => match extract_command_prefix(command) {
        PrefixResult::Prefix(p) => Some(p),
        PrefixResult::None => None,
        PrefixResult::NeedsReview => return self.evaluate_needs_review_shell(tool_names, cwd),
    },
    ShellKind::PowerShell => match crate::powershell_security::extract_command_prefix(command) {
        crate::powershell_security::PrefixResult::Prefix(p) => Some(p),
        crate::powershell_security::PrefixResult::None => None,
        crate::powershell_security::PrefixResult::NeedsReview => {
            return self.evaluate_needs_review_shell(tool_names, cwd);
        }
    },
};
```

Add helper:

```rust
fn evaluate_needs_review_shell(
    &self,
    tool_names: &[&str],
    cwd: &std::path::Path,
) -> Option<RuleDecision> {
    let mut result = None;
    for rule in &self.rules {
        if tool_names.iter().any(|tool_name| Self::match_name(&rule.tool_name, tool_name))
            && rule.command_prefix.is_none()
            && Self::match_cwd_prefix(rule, cwd)
        {
            result = Some(rule.decision.clone());
        }
    }
    result.or(Some(RuleDecision::Deny))
}
```

When matching a prefix:

```rust
let matches_prefix = match shell_kind {
    ShellKind::Bash => haystack.starts_with(prefix),
    ShellKind::PowerShell => haystack.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()),
};
if !matches_prefix {
    continue;
}
```

Update all existing `evaluate_shell_call` call sites/tests to pass `ShellKind::Bash`.

In `core/src/executor/invoke.rs`, import `ShellKind` and route:

```rust
let shell_kind = match canonical_name.as_str() {
    "Bash" => Some(ShellKind::Bash),
    "PowerShell" => Some(ShellKind::PowerShell),
    _ => None,
};
```

Use `shell_kind` instead of `is_shell_tool`.

- [ ] **Step 4: Run permission tests to verify they pass**

Run:

```bash
cargo test -p telos_agent permissions::tests --lib
cargo test -p telos_agent tool_permission_tests
```

Expected: permission unit tests pass; integration PowerShell test may still fail until `PowerShellTool` exists. If it fails only because the tool is not registered, leave that failure for Task 6.

- [ ] **Step 5: Commit only if all tests in this task are green or the only remaining failure is the planned missing tool**

If `core/tests/tool_permission_tests.rs` failure is only missing `PowerShellTool`, do not commit that integration test yet; keep it unstaged for Task 6. Commit permission engine changes:

```bash
git add core/src/permissions.rs core/src/executor/invoke.rs
git commit -m "feat: route shell permissions by shell kind"
```

---

### Task 4: Add PowerShell Read-Only, Dangerous Pattern, Path, And Analyzer Modules

**Files:**
- Create: `core/src/powershell_security/dangerous_cmdlets.rs`
- Create: `core/src/powershell_security/read_only.rs`
- Create: `core/src/powershell_security/path_validation.rs`
- Create: `core/src/powershell_security/analyzer.rs`
- Modify: `core/src/powershell_security/mod.rs`

- [ ] **Step 1: Write failing analyzer tests**

Create `core/src/powershell_security/analyzer.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_needs_review(command: &str) {
        assert!(matches!(analyze(command), CommandSafety::NeedsReview { .. }), "{command}");
    }

    fn assert_safe(command: &str) {
        assert_eq!(analyze(command), CommandSafety::Safe, "{command}");
    }

    #[test]
    fn allows_simple_read_only_commands() {
        assert_safe("Get-Process -Name pwsh");
        assert_safe("Get-Content ./Cargo.toml");
        assert_safe("Select-String -Path ./Cargo.toml -Pattern telos");
    }

    #[test]
    fn asks_for_dangerous_execution_patterns() {
        assert_needs_review("Invoke-Expression 'Get-Process'");
        assert_needs_review("iex (Invoke-WebRequest https://example.com)");
        assert_needs_review("pwsh -EncodedCommand AAAA");
        assert_needs_review("Start-Process powershell -Verb RunAs");
        assert_needs_review("powershell -ExecutionPolicy Bypass -File script.ps1");
    }

    #[test]
    fn asks_for_dangerous_mutation_patterns() {
        assert_needs_review("Remove-Item -Recurse -Force ./target");
        assert_needs_review("Set-Content $PROFILE 'payload'");
        assert_needs_review("Register-ScheduledTask -TaskName x -Action y");
        assert_needs_review("New-Service -Name x -BinaryPathName y");
        assert_needs_review("Set-MpPreference -DisableRealtimeMonitoring $true");
    }

    #[test]
    fn asks_for_assignments_and_redirections() {
        assert_needs_review("$x = Get-Process");
        assert_needs_review("Get-Process > out.txt");
    }
}
```

- [ ] **Step 2: Run analyzer tests to verify they fail**

Run:

```bash
cargo test -p telos_agent powershell_security::analyzer --lib
```

Expected: FAIL because analyzer and `CommandSafety` are missing.

- [ ] **Step 3: Implement conservative analyzer**

Update `core/src/powershell_security/mod.rs`:

```rust
pub mod analyzer;
pub mod dangerous_cmdlets;
pub mod path_validation;
pub mod read_only;

pub use analyzer::{CommandSafety, analyze};
```

Create `dangerous_cmdlets.rs`:

```rust
pub const DANGEROUS_COMMANDS: &[&str] = &[
    "Invoke-Expression",
    "Register-ScheduledTask",
    "New-Service",
    "Set-MpPreference",
];

pub const POWERSHELL_EXECUTABLES: &[&str] = &["pwsh", "pwsh.exe", "powershell", "powershell.exe"];
```

Create `read_only.rs`:

```rust
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
```

Create `path_validation.rs`:

```rust
pub fn has_write_redirection(command: &str) -> bool {
    command.contains('>') && !command.contains("*>$null") && !command.contains("> $null")
}

pub fn has_assignment(command: &str) -> bool {
    let trimmed = command.trim_start();
    trimmed.starts_with('$') && trimmed.contains('=')
}
```

Create `analyzer.rs`:

```rust
use crate::powershell_security::aliases::canonical_command_name;
use crate::powershell_security::dangerous_cmdlets::{DANGEROUS_COMMANDS, POWERSHELL_EXECUTABLES};
use crate::powershell_security::parser;
use crate::powershell_security::path_validation::{has_assignment, has_write_redirection};
use crate::powershell_security::read_only::is_read_only_command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSafety {
    Safe,
    NeedsReview { reason: String },
}

pub fn analyze(command: &str) -> CommandSafety {
    if has_assignment(command) {
        return review("PowerShell assignment requires review");
    }
    if has_write_redirection(command) {
        return review("PowerShell output redirection requires review");
    }
    let parsed = match parser::parse(command) {
        Ok(parsed) => parsed,
        Err(reason) => return review(format!("PowerShell parse failed: {reason}")),
    };
    let commands = parsed.commands();
    if commands.is_empty() {
        return CommandSafety::Safe;
    }
    for cmd in commands {
        if cmd.dynamic {
            return review("dynamic PowerShell command requires review");
        }
        let canonical = canonical_command_name(&cmd.name);
        if DANGEROUS_COMMANDS.iter().any(|name| canonical.eq_ignore_ascii_case(name)) {
            return review(format!("{canonical} requires review"));
        }
        if POWERSHELL_EXECUTABLES.iter().any(|name| cmd.name.eq_ignore_ascii_case(name)) {
            if cmd.args.iter().any(|arg| is_encoded_or_bypass(arg)) {
                return review("nested PowerShell encoded or bypass command requires review");
            }
            return review("nested PowerShell process requires review");
        }
        if canonical.eq_ignore_ascii_case("Start-Process")
            && args_contain_pair(&cmd.args, "-Verb", "RunAs")
        {
            return review("Start-Process -Verb RunAs requires review");
        }
        if canonical.eq_ignore_ascii_case("Remove-Item")
            && has_flag(&cmd.args, "-Recurse")
            && has_flag(&cmd.args, "-Force")
        {
            return review("Remove-Item -Recurse -Force requires review");
        }
        if cmd.args.iter().any(|arg| arg.eq_ignore_ascii_case("$PROFILE")) {
            return review("PowerShell profile writes require review");
        }
        if !is_read_only_command(cmd) {
            return review(format!("{canonical} is not provably read-only"));
        }
    }
    CommandSafety::Safe
}

fn review(reason: impl Into<String>) -> CommandSafety {
    CommandSafety::NeedsReview { reason: reason.into() }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg.eq_ignore_ascii_case(flag))
}

fn args_contain_pair(args: &[String], key: &str, value: &str) -> bool {
    args.windows(2).any(|pair| {
        pair[0].eq_ignore_ascii_case(key) && pair[1].eq_ignore_ascii_case(value)
    })
}

fn is_encoded_or_bypass(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    lower.starts_with("-enc")
        || lower == "-e"
        || lower == "-encodedcommand"
        || lower == "-executionpolicy"
        || lower == "bypass"
}
```

- [ ] **Step 4: Run analyzer tests to verify they pass**

Run:

```bash
cargo test -p telos_agent powershell_security --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/src/powershell_security/mod.rs core/src/powershell_security/analyzer.rs core/src/powershell_security/dangerous_cmdlets.rs core/src/powershell_security/read_only.rs core/src/powershell_security/path_validation.rs
git commit -m "feat: analyze powershell command safety"
```

---

### Task 5: Add PowerShell Executable Discovery And Encoding Helpers

**Files:**
- Create: `core/src/tools/powershell.rs`
- Modify: `core/src/tools/mod.rs`

- [ ] **Step 1: Write failing helper tests**

Create `core/src/tools/powershell.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_powershell_edition_from_path() {
        assert_eq!(PowerShellEdition::from_path("pwsh"), PowerShellEdition::Core);
        assert_eq!(PowerShellEdition::from_path("C:\\Program Files\\PowerShell\\7\\pwsh.exe"), PowerShellEdition::Core);
        assert_eq!(PowerShellEdition::from_path("powershell.exe"), PowerShellEdition::Desktop);
    }

    #[test]
    fn encoded_command_uses_utf16le_base64() {
        assert_eq!(encode_powershell_command("Write-Output hi"), "VwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIABoAGkA");
    }

    #[test]
    fn build_args_use_noninteractive_no_profile_command() {
        assert_eq!(
            build_powershell_args("Get-Process"),
            vec!["-NoProfile", "-NonInteractive", "-Command", "Get-Process"]
        );
    }
}
```

- [ ] **Step 2: Run helper tests to verify they fail**

Run:

```bash
cargo test -p telos_agent tools::powershell --lib
```

Expected: FAIL because module and helpers are missing.

- [ ] **Step 3: Implement helpers and empty tool shell**

Modify `core/src/tools/mod.rs`:

```rust
mod powershell;
pub use powershell::PowerShellTool;
```

Create `core/src/tools/powershell.rs`:

```rust
//! `PowerShell` tool — run PowerShell commands in the workspace cwd.

use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{optional_usize_any, required_string};

pub struct PowerShellTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerShellEdition {
    Core,
    Desktop,
}

impl PowerShellEdition {
    pub fn from_path(path: &str) -> Self {
        let base = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
        if base.trim_end_matches(".exe") == "pwsh" {
            Self::Core
        } else {
            Self::Desktop
        }
    }
}

pub fn encode_powershell_command(command: &str) -> String {
    let mut bytes = Vec::with_capacity(command.len() * 2);
    for unit in command.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn build_powershell_args(command: &str) -> Vec<String> {
    vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-Command".into(),
        command.into(),
    ]
}

#[async_trait]
impl Tool for PowerShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "PowerShell".into(),
            description: "Run a PowerShell command in the current working directory. Prefer Read/Edit/Write/Glob/Grep for file operations.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "description": { "type": "string" },
                    "timeout_ms": { "type": "integer", "description": "Maximum runtime in milliseconds. Defaults to 120000." }
                },
                "required": ["command"]
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some("Use the PowerShell tool for Windows-native shell commands. Prefer Read, Edit, Write, Glob, or Grep for file operations. Use PowerShell syntax, not Bash syntax. Provide a short `description` summarizing the command's intent.")
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "command").map(|_| ())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        let command = required_string(arguments, "command")?;
        match crate::powershell_security::analyze(command) {
            crate::powershell_security::CommandSafety::Safe => Ok(PermissionDecision::Allow),
            crate::powershell_security::CommandSafety::NeedsReview { reason } => {
                Ok(PermissionDecision::Ask {
                    reason: format!("PowerShell command needs review: {reason}"),
                })
            }
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let _command = required_string(&arguments, "command")?;
        let _timeout_ms = optional_usize_any(&arguments, &["timeout_ms"]).unwrap_or(120_000);
        Err(AgentError::ToolExecution {
            tool: "PowerShell".into(),
            message: "PowerShell execution is not implemented yet".into(),
        })
    }
}
```

- [ ] **Step 4: Run helper tests to verify they pass**

Run:

```bash
cargo test -p telos_agent tools::powershell --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/src/tools/mod.rs core/src/tools/powershell.rs
git commit -m "feat: add powershell tool helpers"
```

---

### Task 6: Implement PowerShellTool Registration, Execution, Env Isolation, And Timeout

**Files:**
- Modify: `core/src/tools/mod.rs`
- Modify: `core/src/tools/powershell.rs`
- Modify: `core/src/lib.rs`
- Create: `core/tests/tool_powershell_tests.rs`
- Modify: `core/tests/tool_permission_tests.rs` if Task 3 deferred the integration test

- [ ] **Step 1: Write failing integration tests**

Create `core/tests/tool_powershell_tests.rs`:

```rust
mod common;

use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use telos_agent::*;

fn ctx(cwd: std::path::PathBuf, env: HashMap<String, String>) -> ToolContext {
    ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd,
        env,
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        timeout: None,
        max_file_read_bytes: usize::MAX,
    }
}

fn powershell_available() -> bool {
    std::process::Command::new("pwsh").arg("-NoProfile").arg("-Command").arg("$PSVersionTable.PSVersion").output().is_ok()
        || std::process::Command::new("powershell").arg("-NoProfile").arg("-Command").arg("$PSVersionTable.PSVersion").output().is_ok()
}

#[test]
fn register_core_tools_includes_powershell() {
    let mut tools = ToolRegistry::new();
    register_core_tools(&mut tools);
    assert!(tools.get("PowerShell").is_some());
}

#[tokio::test]
async fn safe_command_runs_with_clean_environment_when_powershell_exists() {
    if !powershell_available() {
        eprintln!("skipping: PowerShell not installed");
        return;
    }
    unsafe { std::env::set_var("TELOS_AGENT_SECRET", "leaked") };
    let tool = PowerShellTool;
    let mut env = HashMap::new();
    env.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    let output = tool
        .invoke(
            json!({ "command": "Write-Output $env:TELOS_AGENT_SECRET" }),
            ctx(std::env::temp_dir(), env),
        )
        .await
        .unwrap();
    assert_eq!(output.content["stdout"].as_str().unwrap().trim(), "");
    unsafe { std::env::remove_var("TELOS_AGENT_SECRET") };
}

#[tokio::test]
async fn configured_env_is_passed_when_powershell_exists() {
    if !powershell_available() {
        eprintln!("skipping: PowerShell not installed");
        return;
    }
    let tool = PowerShellTool;
    let mut env = HashMap::new();
    env.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    env.insert("MY_PS_VAR".into(), "present".into());
    let output = tool
        .invoke(
            json!({ "command": "Write-Output $env:MY_PS_VAR" }),
            ctx(std::env::temp_dir(), env),
        )
        .await
        .unwrap();
    assert_eq!(output.content["stdout"].as_str().unwrap().trim(), "present");
}

#[tokio::test]
async fn timeout_returns_error_when_powershell_exists() {
    if !powershell_available() {
        eprintln!("skipping: PowerShell not installed");
        return;
    }
    let tool = PowerShellTool;
    let mut env = HashMap::new();
    env.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    let err = tool
        .invoke(
            json!({ "command": "Start-Sleep -Seconds 5", "timeout_ms": 10 }),
            ctx(std::env::temp_dir(), env),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("timed out"));
}
```

- [ ] **Step 2: Run integration tests to verify they fail**

Run:

```bash
cargo test -p telos_agent tool_powershell_tests
```

Expected: FAIL because `PowerShellTool` is not registered and execution returns not implemented.

- [ ] **Step 3: Implement registration, discovery, and execution**

In `core/src/tools/mod.rs`, register:

```rust
registry.register(PowerShellTool);
```

In `core/src/lib.rs`, add `PowerShellTool` to the built-in tools re-export list.

In `core/src/tools/powershell.rs`, add:

```rust
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

pub fn find_powershell_executable() -> Option<String> {
    if let Ok(path) = std::env::var("TELOS_POWERSHELL_PATH")
        && !path.trim().is_empty()
    {
        return Some(path);
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["pwsh.exe", "powershell.exe"]
    } else {
        &["pwsh", "powershell"]
    };
    candidates.iter().find(|candidate| executable_exists(candidate)).map(|s| (*s).into())
}

fn executable_exists(candidate: &str) -> bool {
    std::process::Command::new(candidate)
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("$PSVersionTable.PSVersion")
        .output()
        .is_ok()
}
```

Replace `invoke` with implementation matching `ShellTool`, using `find_powershell_executable`, `build_powershell_args`, `env_clear`, `envs(context.env.iter())`, `timeout`, and `run_powershell_child`.

Use this child runner:

```rust
async fn run_powershell_child(mut command: Command) -> std::io::Result<std::process::Output> {
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    child.kill_on_drop(true);
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let status = child.wait().await?;
    let stdout = stdout_task.await.map_err(std::io::Error::other)??;
    let stderr = stderr_task.await.map_err(std::io::Error::other)??;
    Ok(std::process::Output { status, stdout, stderr })
}
```

Copy `trim_large_output` from `shell.rs` or move it to `tools/shared.rs` if you prefer to avoid duplication.

- [ ] **Step 4: Run integration tests to verify they pass**

Run:

```bash
cargo test -p telos_agent tool_powershell_tests
cargo test -p telos_agent permission_engine_allows_powershell_by_command_prefix
```

Expected: PASS. Execution tests print skip messages if no PowerShell binary exists.

- [ ] **Step 5: Commit**

```bash
git add core/src/tools/mod.rs core/src/tools/powershell.rs core/src/lib.rs core/tests/tool_powershell_tests.rs core/tests/tool_permission_tests.rs
git commit -m "feat: add powershell tool execution"
```

---

### Task 7: Add CLI Config For Default Shell

**Files:**
- Modify: `cli/src/config/types.rs`
- Modify: `cli/src/config/merge.rs`
- Modify: `cli/src/config/mod.rs`
- Modify: `cli/tests/cli_tests.rs`

- [ ] **Step 1: Write failing config tests**

Add to existing config tests in `cli/src/config/mod.rs` or `cli/tests/cli_tests.rs`:

```rust
#[test]
fn parses_agent_default_shell() {
    let cfg: telos_cli::config::FileConfig = toml::from_str(
        r#"
        [agent]
        default_shell = "powershell"
        "#,
    )
    .unwrap();
    assert_eq!(
        cfg.agent.unwrap().default_shell,
        Some(telos_cli::config::DefaultShell::PowerShell)
    );
}

#[test]
fn merge_configs_project_default_shell_overrides_user() {
    let user = telos_cli::config::FileConfig {
        agent: Some(telos_cli::config::AgentSection {
            default_shell: Some(telos_cli::config::DefaultShell::Bash),
            ..Default::default()
        }),
        ..Default::default()
    };
    let project = telos_cli::config::FileConfig {
        agent: Some(telos_cli::config::AgentSection {
            default_shell: Some(telos_cli::config::DefaultShell::PowerShell),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = telos_cli::config::merge_configs(Some(user), Some(project));
    assert_eq!(
        merged.agent.unwrap().default_shell,
        Some(telos_cli::config::DefaultShell::PowerShell)
    );
}
```

- [ ] **Step 2: Run config tests to verify they fail**

Run:

```bash
cargo test -p telos-cli parses_agent_default_shell merge_configs_project_default_shell_overrides_user
```

Expected: FAIL because `DefaultShell` and `default_shell` do not exist.

- [ ] **Step 3: Implement config type and merge**

In `cli/src/config/types.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DefaultShell {
    Bash,
    PowerShell,
}
```

Add field to `AgentSection`:

```rust
pub default_shell: Option<DefaultShell>,
```

In `cli/src/config/merge.rs`, include `default_shell` in all `AgentSection` constructions:

```rust
default_shell: u.default_shell,
default_shell: p.default_shell,
default_shell: p.default_shell.or(u.default_shell),
```

In `cli/src/config/mod.rs`, re-export `DefaultShell`.

- [ ] **Step 4: Run config tests to verify they pass**

Run:

```bash
cargo test -p telos-cli parses_agent_default_shell merge_configs_project_default_shell_overrides_user
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add cli/src/config/types.rs cli/src/config/merge.rs cli/src/config/mod.rs cli/tests/cli_tests.rs
git commit -m "feat: add default shell config"
```

---

### Task 8: Full Verification And Cleanup

**Files:**
- Modify only files needed for compile/test cleanup.

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt
```

Expected: exits 0.

- [ ] **Step 2: Run core test suite**

Run:

```bash
cargo test -p telos_agent
```

Expected: PASS.

- [ ] **Step 3: Run CLI config tests**

Run:

```bash
cargo test -p telos-cli
```

Expected: PASS.

- [ ] **Step 4: Run full workspace tests if time allows**

Run:

```bash
cargo test --workspace
```

Expected: PASS, or document any unrelated pre-existing failure.

- [ ] **Step 5: Check git diff for unrelated changes**

Run:

```bash
git status --short
git diff --stat
```

Expected: only PowerShell migration files are modified, plus pre-existing unrelated files that must not be touched.

- [ ] **Step 6: Commit cleanup**

If formatting or cleanup changed files:

```bash
git add core cli Cargo.lock
git commit -m "test: verify powershell tool migration"
```

If there are no cleanup changes, skip this commit.

---

## Self-Review

- Spec coverage: This plan covers parser dependency, PowerShell tool execution, permission separation, prefix extraction, analyzer/read-only/path modules, registration, and config.
- No placeholders: Every task has concrete files, test names, commands, and implementation targets.
- Type consistency: `PowerShellTool`, `ShellKind::PowerShell`, `DefaultShell::PowerShell`, and `tree-sitter-pwsh` are used consistently.
- Risk note: The parser wrapper begins with a reduced AST on top of tree-sitter. Follow-up work can deepen node-level extraction while tests preserve the behavior added here.
