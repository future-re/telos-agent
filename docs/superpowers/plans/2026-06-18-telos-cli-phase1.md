# telos-cli Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the bare-bones telos-cli into a Codex-grade terminal experience: config-file support, rustyline REPL with slash commands, markdown rendering, approval policies, session persistence, and project context detection.

**Architecture:** Extend the existing `telos-cli` crate with new modules alongside the current `cli.rs`, `config.rs`, `runner.rs`, `terminal.rs`, and `approval.rs`. The REPL moves from a raw stdin loop to rustyline with history, completion, and Emacs/Vi keybindings. Configuration flows from CLI flags → env vars → `.telos.toml` (project) → `~/.config/telos/config.toml` (user). Agent responses render through termimad for rich markdown display.

**Tech Stack:** Rust, rustyline, termimad, dissimilar, glob, toml, serde, dirs, clap (existing), tokio (existing)

---

## File map

| File | Responsibility |
|------|----------------|
| `telos-cli/Cargo.toml` | New dependencies: rustyline, termimad, dissimilar, glob, toml, dirs |
| `telos-cli/src/lib.rs` | Module declarations, updated `run()` orchestration |
| `telos-cli/src/main.rs` | Binary entry point (unchanged) |
| `telos-cli/src/cli.rs` | Clap CLI — add `--config` flag, new slash-command variants |
| `telos-cli/src/config.rs` | Extended: load toml config files, merge layers |
| `telos-cli/src/project.rs` | NEW: project root detection, .telos.toml discovery |
| `telos-cli/src/session.rs` | NEW: session persistence (save/load chat history) |
| `telos-cli/src/display.rs` | NEW: termimad markdown rendering, diff coloring |
| `telos-cli/src/repl.rs` | NEW: rustyline REPL with history, completion, slash commands |
| `telos-cli/src/approval.rs` | Extended: approval policy system |
| `telos-cli/src/runner.rs` | Updated: wire display + session + policy into run loops |
| `telos-cli/src/terminal.rs` | (unchanged, keep existing) |
| `telos-cli/README.md` | Updated usage docs |
| `telos-cli/tests/cli_tests.rs` | New integration tests for all Phase 1 features |

---

### Task 1: Add crate dependencies

**Files:**
- Modify: `telos-cli/Cargo.toml`
- Test: `telos-cli/tests/cli_tests.rs` (compile-check test)

- [ ] **Step 1: Write the failing test**

Add a smoke test that verifies the new crates are importable:

```rust
#[test]
fn new_dependencies_compile() {
    // If this module compiles, all crates resolved correctly.
    // rustyline
    let _ = rustyline::Editor::<()>::new();
    // termimad
    let _ = termimad::MadSkin::default();
    // toml
    let _ = toml::Table::new();
    // dirs
    let _ = dirs::config_dir();
    // glob
    let _ = glob::glob("*.rs");
    // dissimilar
    let _ = dissimilar::diff("a", "b");
}
```

- [ ] **Step 2: Add crates to Cargo.toml**

```toml
[dependencies]
# ... existing deps above ...
dirs = "5"
dissimilar = "1"
glob = "0.3"
rustyline = { version = "14", default-features = false, features = ["with-file-history"] }
termimad = "0.30"
toml = "0.8"
```

- [ ] **Step 3: Verify**

```bash
cargo test -p telos-cli -- new_dependencies_compile
cargo check -p telos-cli 2>&1 | grep -v "warning"
```

- **Commit:** `feat(telos-cli): add rustyline, termimad, dissimilar, glob, toml, dirs deps`

---

### Task 2: Configuration file support (toml + dirs)

**Files:**
- Modify: `telos-cli/src/config.rs`
- Modify: `telos-cli/src/cli.rs` (add `--config` flag)
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write the failing test**

```rust
use telos_cli::config::FileConfig;

#[test]
fn parses_telos_toml() {
    let toml_str = r#"
[agent]
model = "deepseek-chat"
max_iterations = 16

[display]
theme = "dark"
render_markdown = true

[approval]
default_policy = "ask"
"#;
    let cfg: FileConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.agent.as_ref().unwrap().model.as_deref(), Some("deepseek-chat"));
    assert_eq!(cfg.agent.as_ref().unwrap().max_iterations, Some(16));
    assert_eq!(cfg.display.as_ref().unwrap().theme.as_deref(), Some("dark"));
    assert!(cfg.display.as_ref().unwrap().render_markdown.unwrap());
    assert_eq!(cfg.approval.as_ref().unwrap().default_policy.as_deref(), Some("ask"));
}
```

- [ ] **Step 2: Define FileConfig structs**

Add to `config.rs`:

```rust
use serde::Deserialize;

/// Configuration loaded from toml files (user-level ~/.config/telos/config.toml
/// and project-level .telos.toml).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FileConfig {
    pub agent: Option<AgentSection>,
    pub display: Option<DisplaySection>,
    pub approval: Option<ApprovalSection>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub max_iterations: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DisplaySection {
    pub theme: Option<String>,
    pub render_markdown: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalSection {
    pub default_policy: Option<String>,
    pub policies: Option<HashMap<String, String>>,
}

/// Load config from user path ~/.config/telos/config.toml.
pub fn load_user_config() -> Result<Option<FileConfig>> { ... }

/// Load config from a project .telos.toml.
pub fn load_project_config(dir: &Path) -> Result<Option<FileConfig>> { ... }

/// Merge layers: project overrides user overrides defaults.
pub fn merge_configs(user: Option<FileConfig>, project: Option<FileConfig>) -> FileConfig { ... }
```

- [ ] **Step 3: Add --config CLI flag**

In `cli.rs`:
```rust
/// Path to a config file to load.
#[clap(long, env = "TELOS_CONFIG", global = true)]
pub config: Option<PathBuf>,
```

- [ ] **Step 4: Implement & verify**

```bash
cargo test -p telos-cli -- parses_telos_toml
cargo test -p telos-cli
```

- **Commit:** `feat(telos-cli): add toml config file support with user/project layers`

---

### Task 3: Project context detection

**Files:**
- Create: `telos-cli/src/project.rs`
- Modify: `telos-cli/src/lib.rs` (add `pub mod project;`)
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn detects_project_root_via_git() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    // .git is the anchor
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let found = telos_cli::project::find_project_root(dir.path().join("src")).unwrap();
    assert_eq!(found, dir.path().canonicalize().unwrap());
}

#[test]
fn detects_project_root_via_telos_toml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".telos.toml"), "[agent]\nmodel = \"test\"").unwrap();
    let found = telos_cli::project::find_project_root(dir.path()).unwrap();
    assert_eq!(found, dir.path().canonicalize().unwrap());
}
```

- [ ] **Step 2: Implement project.rs**

```rust
use std::path::{Path, PathBuf};

/// Walk upward from `start_dir` looking for a project root marker:
/// 1. `.telos.toml` (strongest signal)
/// 2. `.git` directory or file (worktree)
/// Returns the first directory containing a marker, or `start_dir` if none found.
pub fn find_project_root(start_dir: impl AsRef<Path>) -> std::io::Result<PathBuf> { ... }

/// Read `.telos.toml` from the project root, if one exists.
pub fn read_project_config(project_root: &Path) -> Option<crate::config::FileConfig> { ... }
```

- [ ] **Step 3: Verify**

```bash
cargo test -p telos-cli -- detects_project_root
cargo test -p telos-cli
```

- **Commit:** `feat(telos-cli): add project root detection via .git and .telos.toml`

---

### Task 4: Expanded slash commands

**Files:**
- Modify: `telos-cli/src/runner.rs` (expand `ReplCommand` enum)
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn parse_add_command() {
    assert!(matches!(
        telos_cli::runner::parse_repl_command("/add src/*.rs"),
        telos_cli::runner::ReplCommand::Add(pattern) if pattern == "src/*.rs"
    ));
}

#[test]
fn parse_drop_command() {
    assert!(matches!(
        telos_cli::runner::parse_repl_command("/drop src/old.rs"),
        telos_cli::runner::ReplCommand::Drop(pattern) if pattern == "src/old.rs"
    ));
}

#[test]
fn parse_clear_command() {
    assert!(matches!(
        telos_cli::runner::parse_repl_command("/clear"),
        telos_cli::runner::ReplCommand::Clear
    ));
}

#[test]
fn parse_help_command() {
    assert!(matches!(
        telos_cli::runner::parse_repl_command("/help"),
        telos_cli::runner::ReplCommand::Help
    ));
}
```

- [ ] **Step 2: Expand ReplCommand**

```rust
#[derive(Debug, PartialEq)]
pub enum ReplCommand {
    Exit,
    Reset,
    Tools,
    Clear,
    Help,
    Add(String),
    Drop(String),
    Model(String),
    Chat(String),
}

pub fn parse_repl_command(input: &str) -> ReplCommand {
    let input = input.trim();
    match input {
        "/exit" | "/quit" => ReplCommand::Exit,
        "/reset" => ReplCommand::Reset,
        "/tools" => ReplCommand::Tools,
        "/clear" => ReplCommand::Clear,
        "/help" => ReplCommand::Help,
        s if s.starts_with("/add ") => ReplCommand::Add(s[5..].trim().to_string()),
        s if s.starts_with("/drop ") => ReplCommand::Drop(s[6..].trim().to_string()),
        s if s.starts_with("/model ") => ReplCommand::Model(s[7..].trim().to_string()),
        _ => ReplCommand::Chat(input.to_string()),
    }
}
```

- [ ] **Step 3: Implement handler stubs in runner**

Handle `Clear`, `Help`, `Add`, `Drop`, `Model` in the REPL loop — at least print user-facing messages for now.

- [ ] **Step 4: Verify**

```bash
cargo test -p telos-cli -- parse_
cargo test -p telos-cli
```

- **Commit:** `feat(telos-cli): expand slash commands with /add, /drop, /clear, /help, /model`

---

### Task 5: Approval policy system

**Files:**
- Modify: `telos-cli/src/approval.rs`
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
use telos_cli::approval::{ApprovalPolicy, PolicyConfig};

#[test]
fn policy_always_allow() {
    let policy = ApprovalPolicy::AlwaysAllow;
    let decision = policy.decide("bash", "echo hello".into());
    assert!(matches!(decision, telos_agent::ApprovalDecision::Allow));
}

#[test]
fn policy_always_ask() {
    let policy = ApprovalPolicy::AlwaysAsk;
    // AlwaysAsk returns None, meaning "delegate to interactive handler"
    let decision = policy.decide("bash", "echo hello".into());
    assert!(decision.is_none());
}

#[test]
fn policy_per_tool_allow() {
    let mut policies = std::collections::HashMap::new();
    policies.insert("read".to_string(), ApprovalPolicy::AlwaysAllow);
    policies.insert("write".to_string(), ApprovalPolicy::AlwaysAsk);
    let config = PolicyConfig { default: ApprovalPolicy::AlwaysAsk, policies };

    assert!(config.policy_for("read").is_allow());
    assert!(!config.policy_for("write").is_allow());
    assert!(!config.policy_for("bash").is_allow()); // falls to default
}
```

- [ ] **Step 2: Implement ApprovalPolicy enum**

```rust
/// Approval policy for tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    /// Always allow without prompting.
    AlwaysAllow,
    /// Always prompt interactively.
    AlwaysAsk,
    /// Always deny without prompting.
    AlwaysDeny,
}

impl ApprovalPolicy {
    pub fn is_allow(self) -> bool { matches!(self, Self::AlwaysAllow) }
    pub fn decide(self, tool_name: &str, args: serde_json::Value) -> Option<ApprovalDecision> { ... }
}

/// Per-tool policy configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolicyConfig {
    pub default: ApprovalPolicy,
    pub policies: HashMap<String, ApprovalPolicy>,
}

impl PolicyConfig {
    pub fn policy_for(&self, tool_name: &str) -> ApprovalPolicy { ... }
}
```

- [ ] **Step 3: Integrate into TerminalApprovalHandler**

Store a `PolicyConfig` inside the handler; fast-path Allow/Deny before showing the prompt.

- [ ] **Step 4: Verify**

```bash
cargo test -p telos-cli -- policy
cargo test -p telos-cli
```

- **Commit:** `feat(telos-cli): add configurable approval policy system`

---

### Task 6: Markdown display via termimad

**Files:**
- Create: `telos-cli/src/display.rs`
- Modify: `telos-cli/src/lib.rs` (add `pub mod display;`)
- Modify: `telos-cli/src/runner.rs` (use display for assistant output)
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn termimad_renders_markdown() {
    let skin = termimad::MadSkin::default();
    let md = "# Hello\n\n**bold** and `code`";
    let rendered = skin.term_text(&skin.text(md, None));
    // The rendered string should contain ANSI escape codes (bold, headers, etc.)
    assert!(!rendered.is_empty());
    assert!(rendered.contains("\x1b[")); // ANSI escape
}

#[test]
fn display_module_noop_without_markdown() {
    let text = "plain text\nno formatting";
    let rendered = telos_cli::display::render(text, false);
    // Without markdown, just returns the text (possibly with ANSI reset)
    assert!(rendered.contains("plain text"));
}
```

- [ ] **Step 2: Implement display.rs**

```rust
use termimad::MadSkin;

/// Render markdown text as ANSI terminal output.
pub fn render(text: &str, markdown_enabled: bool) -> String {
    if !markdown_enabled {
        return text.to_string();
    }
    let skin = MadSkin::default();
    let rendered = skin.term_text(&skin.text(text, None));
    rendered.to_string()
}

/// Render a diff between two strings with ANSI coloring.
pub fn render_diff(old: &str, new: &str) -> String {
    let chunks = dissimilar::diff(old, new);
    let mut out = String::new();
    for chunk in chunks {
        match chunk {
            dissimilar::Chunk::Equal(s) => out.push_str(s),
            dissimilar::Chunk::Delete(s) => {
                out.push_str("\x1b[31m"); // red
                out.push_str(s);
                out.push_str("\x1b[0m");
            }
            dissimilar::Chunk::Insert(s) => {
                out.push_str("\x1b[32m"); // green
                out.push_str(s);
                out.push_str("\x1b[0m");
            }
        }
    }
    out
}
```

- [ ] **Step 3: Wire into runner**

In `run_with_provider`, pipe assistant text through `display::render()` based on config.

- [ ] **Step 4: Verify**

```bash
cargo test -p telos-cli -- termimad
cargo test -p telos-cli -- display
cargo test -p telos-cli
```

- **Commit:** `feat(telos-cli): add markdown rendering via termimad and diff via dissimilar`

---

### Task 7: Session persistence

**Files:**
- Create: `telos-cli/src/session.rs`
- Modify: `telos-cli/src/lib.rs` (add `pub mod session;`)
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
use telos_cli::session::ChatHistory;

#[test]
fn save_and_load_chat_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");

    let mut history = ChatHistory::default();
    history.add_user("hello");
    history.add_assistant("hi there");
    history.add_user("refactor main.rs");
    history.add_assistant("ok, I'll refactor main.rs");

    history.save_to(&path).unwrap();
    let loaded = ChatHistory::load_from(&path).unwrap();
    assert_eq!(loaded.messages.len(), 4);
    assert_eq!(loaded.messages[0].content, "hello");
    assert_eq!(loaded.messages[3].content, "ok, I'll refactor main.rs");
}

#[test]
fn session_auto_names() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_dir = dir.path().join("sessions");
    let name = telos_cli::session::next_session_name(&sessions_dir, "chat");
    assert!(name.starts_with("chat-"));
    assert!(name.ends_with(".json"));
}
```

- [ ] **Step 2: Implement session.rs**

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatHistory {
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

impl ChatHistory {
    pub fn add_user(&mut self, content: impl Into<String>) { ... }
    pub fn add_assistant(&mut self, content: impl Into<String>) { ... }
    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> { ... }
    pub fn load_from(path: &Path) -> anyhow::Result<Self> { ... }
}

/// Find the session directory: `~/.local/share/telos/sessions/` or `<project>/.telos/sessions/`.
pub fn sessions_dir(project_root: Option<&Path>) -> PathBuf { ... }

/// Generate a unique session filename: `<prefix>-<timestamp>.json`.
pub fn next_session_name(dir: &Path, prefix: &str) -> String { ... }
```

- [ ] **Step 3: Verify**

```bash
cargo test -p telos-cli -- session
cargo test -p telos-cli -- chat_history
cargo test -p telos-cli
```

- **Commit:** `feat(telos-cli): add chat session save/load persistence`

---

### Task 8: Rustyline REPL integration

**Files:**
- Create: `telos-cli/src/repl.rs`
- Modify: `telos-cli/src/lib.rs` (add `pub mod repl;`)
- Modify: `telos-cli/src/runner.rs` (replace stdin loop with rustyline)
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn rustyline_editor_creates() {
    let mut editor = rustyline::Editor::<()>::new().unwrap();
    editor.set_max_history_size(1000).unwrap();
    // Can't easily test readline in unit tests, but we can test creation.
}

#[test]
fn repl_completer_registers_commands() {
    let mut editor = telos_cli::repl::build_editor().unwrap();
    // Verify helper is registered (we can check by attempting completion)
    let completions = telos_cli::repl::complete_command("/", "");
    assert!(completions.iter().any(|(cmd, _)| cmd == "/help"));
    assert!(completions.iter().any(|(cmd, _)| cmd == "/exit"));
    assert!(completions.iter().any(|(cmd, _)| cmd == "/add"));
}
```

- [ ] **Step 2: Implement repl.rs**

```rust
use rustyline::{Editor, history::FileHistory, Config, CompletionType, EditMode};

/// Build a rustyline Editor pre-configured for telos.
pub fn build_editor() -> rustyline::Result<Editor<()>> {
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();

    let mut editor = Editor::with_config(config)?;
    editor.set_max_history_size(1000)?;

    // Load history from ~/.local/share/telos/history.txt via dirs.
    if let Some(data_dir) = dirs::data_dir() {
        let history_path = data_dir.join("telos").join("history.txt");
        let _ = editor.load_history(&history_path.to_string_lossy());
    }

    Ok(editor)
}

/// Returns a list of (command, description) pairs for tab completion.
pub fn complete_command(prefix: &str, partial: &str) -> Vec<(String, String)> {
    let commands: &[(&str, &str)] = &[
        ("/exit", "Exit the REPL"),
        ("/quit", "Exit the REPL"),
        ("/reset", "Reset the conversation"),
        ("/clear", "Clear the screen"),
        ("/tools", "List available tools"),
        ("/help", "Show help"),
        ("/add", "Add files to context (glob)"),
        ("/drop", "Remove files from context"),
        ("/model", "Change model"),
    ];
    commands.iter()
        .filter(|(cmd, _)| cmd.starts_with(partial))
        .map(|(cmd, desc)| (cmd.to_string(), desc.to_string()))
        .collect()
}
```

- [ ] **Step 3: Replace stdin loop in runner.rs**

In `run_chat`, swap the raw `tokio::io::BufReader::new(tokio::io::stdin())` loop with a rustyline readline loop:

```rust
let mut editor = crate::repl::build_editor()?;
let prompt = "telos> ";
loop {
    match editor.readline(prompt) {
        Ok(line) => {
            let input = line.trim();
            if input.is_empty() { continue; }
            editor.add_history_entry(input)?;
            match parse_repl_command(input) { ... }
        }
        Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => break,
        Err(e) => return Err(e.into()),
    }
}
```

- [ ] **Step 4: Verify**

```bash
cargo test -p telos-cli -- repl
cargo test -p telos-cli -- rustyline
cargo test -p telos-cli
```

- **Commit:** `feat(telos-cli): replace stdin loop with rustyline REPL`

---

### Task 9: Runner integration — wire everything together

**Files:**
- Modify: `telos-cli/src/runner.rs`
- Modify: `telos-cli/src/lib.rs`
- New test in: `telos-cli/tests/cli_tests.rs`

- [ ] **Step 1: Write integration test**

```rust
#[test]
fn full_pipeline_mock_chat() {
    // Test that run_chat with mock provider starts up and processes commands
    // We can't easily test rustyline in automated tests, but we test the setup path.
    let options = telos_cli::cli::SharedOptions {
        provider: Some(telos_cli::cli::ProviderArg::Mock),
        model: None,
        api_key: None,
        cwd: None,
        max_iterations: 1,
        no_validate_schema: false,
        quiet: false,
        verbose: false,
        config: None,
    };
    // Test that setup_session succeeds
    let config = telos_cli::config::build_agent_config(&options, None).unwrap();
    assert_eq!(config.max_iterations, 1);
}
```

- [ ] **Step 2: Wire config layers in run()**

In `lib.rs`:
1. Load user config from `~/.config/telos/config.toml`
2. Detect project root, load `.telos.toml` if present
3. Merge configs
4. Apply to CLI options (where CLI flag is not set)

- [ ] **Step 3: Wire display + policy + session into runner**

- Pass `markdown_enabled` flag through to display
- Initialize `PolicyConfig` and pass to approval handler
- At session start, offer to resume previous session

- [ ] **Step 4: Verify everything compiles and tests pass**

```bash
cargo test -p telos-cli
cargo build -p telos-cli --release
```

- **Commit:** `feat(telos-cli): integrate config layers, display, policy, and session into runner`

---

### Task 10: Documentation update

**Files:**
- Modify: `telos-cli/README.md`
- Modify: `README.md` (workspace root, if needed)

- [ ] **Step 1: Update telos-cli/README.md**

- Document new config file format
- Document slash commands
- Document session persistence
- Add examples of `~/.config/telos/config.toml` and `.telos.toml`
- Update usage section with new features

- [ ] **Step 2: Verify rendering**

```bash
# Manual check — README.md should be clear and complete
head -20 telos-cli/README.md
```

- [ ] **Step 3: Run final test suite**

```bash
cargo test -p telos-cli
cargo test
cargo clippy -p telos-cli -- -D warnings
```

- **Commit:** `docs(telos-cli): update README with Phase 1 features and examples`
