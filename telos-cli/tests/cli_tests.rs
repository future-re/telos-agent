use assert_cmd::Command;
use predicates::prelude::*;

// ── Task 1: Dependency compile check ────────────────────────────────────────

#[test]
fn new_dependencies_compile() {
    // Verify all Phase 1 crates are importable and basic types construct.
    // rustyline
    let _ = rustyline::Editor::<(), rustyline::history::FileHistory>::new();
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

// ── Existing integration tests ──────────────────────────────────────────────

#[test]
fn runs_single_mock_prompt() {
    let mut cmd = Command::cargo_bin("telos").unwrap();
    cmd.args(["--provider", "mock", "hello"]);
    cmd.assert().success().stdout(predicate::str::contains("Mock provider"));
}

#[test]
fn completion_subcommand_works() {
    let mut cmd = Command::cargo_bin("telos").unwrap();
    cmd.args(["completion", "bash"]);
    cmd.assert().success();
}

// ── Task 2: Config file parsing ─────────────────────────────────────────────

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
    let cfg: telos_cli::config::FileConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.agent.as_ref().unwrap().model.as_deref(), Some("deepseek-chat"));
    assert_eq!(cfg.agent.as_ref().unwrap().max_iterations, Some(16));
    assert_eq!(cfg.display.as_ref().unwrap().theme.as_deref(), Some("dark"));
    assert!(cfg.display.as_ref().unwrap().render_markdown.unwrap());
    assert_eq!(cfg.approval.as_ref().unwrap().default_policy.as_deref(), Some("ask"));
}

#[test]
fn load_user_config_from_test_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"
[agent]
model = "gpt-4"
max_iterations = 32

[display]
theme = "light"

[approval]
default_policy = "deny"
"#,
    )
    .unwrap();

    let cfg = telos_cli::config::load_user_config(Some(&config_path))
        .unwrap()
        .expect("config should be Some");

    assert_eq!(cfg.agent.as_ref().unwrap().model.as_deref(), Some("gpt-4"));
    assert_eq!(cfg.agent.as_ref().unwrap().max_iterations, Some(32));
    assert_eq!(cfg.display.as_ref().unwrap().theme.as_deref(), Some("light"));
    assert_eq!(cfg.approval.as_ref().unwrap().default_policy.as_deref(), Some("deny"));
}

// ── Task 3: Project context detection ───────────────────────────────────────

#[test]
fn detects_project_root_via_git() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join(".git")).unwrap();
    let sub = root.join("deep").join("nested");
    std::fs::create_dir_all(&sub).unwrap();

    let found = telos_cli::find_project_root(&sub).unwrap();
    assert_eq!(found, root.canonicalize().unwrap());
}

#[test]
fn detects_project_root_via_telos_toml() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join(".telos.toml"), "[agent]\nmodel = \"test\"\n").unwrap();
    let sub = root.join("a").join("b");
    std::fs::create_dir_all(&sub).unwrap();

    let found = telos_cli::find_project_root(&sub).unwrap();
    assert_eq!(found, root.canonicalize().unwrap());
}

// ── Task 4: Expanded slash commands ─────────────────────────────────────────

use telos_cli::runner::{ReplCommand, parse_repl_command};

#[test]
fn parse_add_command() {
    assert!(matches!(parse_repl_command("/add src/*.rs"), ReplCommand::Add(p) if p == "src/*.rs"));
}

#[test]
fn parse_drop_command() {
    assert!(
        matches!(parse_repl_command("/drop src/old.rs"), ReplCommand::Drop(p) if p == "src/old.rs")
    );
}

#[test]
fn parse_clear_command() {
    assert!(matches!(parse_repl_command("/clear"), ReplCommand::Clear));
}

#[test]
fn parse_help_command() {
    assert!(matches!(parse_repl_command("/help"), ReplCommand::Help));
}

#[test]
fn parse_model_command() {
    assert!(matches!(parse_repl_command("/model gpt-5"), ReplCommand::Model(m) if m == "gpt-5"));
}

#[test]
fn parse_chat_fallback() {
    assert!(matches!(parse_repl_command("plain text"), ReplCommand::Chat(t) if t == "plain text"));
}

// ── Task 5: Approval policy system ──────────────────────────────────────────

use telos_cli::approval::{ApprovalPolicy, PolicyConfig};

#[test]
fn policy_always_allow_returns_allow() {
    let policy = ApprovalPolicy::AlwaysAllow;
    let decision = policy.decide("bash", serde_json::Value::String("echo hello".into()));
    assert!(matches!(decision, Some(telos_agent::ApprovalDecision::Allow)));
}

#[test]
fn policy_always_deny_returns_deny() {
    let policy = ApprovalPolicy::AlwaysDeny;
    let decision = policy.decide("write", serde_json::Value::String("rm -rf /".into()));
    assert!(matches!(decision, Some(telos_agent::ApprovalDecision::Deny { .. })));
}

#[test]
fn policy_always_ask_returns_none() {
    let policy = ApprovalPolicy::AlwaysAsk;
    let decision = policy.decide("read", serde_json::Value::String("main.rs".into()));
    assert!(decision.is_none());
}

#[test]
fn policy_per_tool_lookup() {
    let mut policies = std::collections::HashMap::new();
    policies.insert("read".to_string(), ApprovalPolicy::AlwaysAllow);
    policies.insert("write".to_string(), ApprovalPolicy::AlwaysDeny);
    let config = PolicyConfig { default: ApprovalPolicy::AlwaysAsk, policies };

    assert!(config.policy_for("read").is_allow());
    assert!(!config.policy_for("write").is_allow());
    assert!(!config.policy_for("bash").is_allow()); // falls to default (AlwaysAsk)
}

// ── Task 6: Markdown display ────────────────────────────────────────────────

#[test]
fn termimad_renders_markdown() {
    let rendered = telos_cli::display::render("# Hello\n\n**bold** and `code`", true);
    assert!(!rendered.is_empty());
    assert!(rendered.contains("\x1b["));
}

#[test]
fn display_render_disabled_returns_plain() {
    let text = "plain text\nno formatting";
    let rendered = telos_cli::display::render(text, false);
    assert_eq!(rendered, text);
}

#[test]
fn render_diff_colors_additions_green() {
    let result = telos_cli::display::render_diff("a", "a\nb");
    assert!(result.contains("\x1b[32m")); // green for insertions
}

#[test]
fn render_diff_colors_removals_red() {
    let result = telos_cli::display::render_diff("a\nb", "a");
    assert!(result.contains("\x1b[31m")); // red for deletions
}

// ── Task 7: Session persistence ─────────────────────────────────────────────

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
    assert_eq!(loaded.messages[0].role, "user");
    assert_eq!(loaded.messages[0].content, "hello");
    assert_eq!(loaded.messages[3].role, "assistant");
    assert_eq!(loaded.messages[3].content, "ok, I'll refactor main.rs");
}

#[test]
fn session_auto_names() {
    let dir = tempfile::tempdir().unwrap();
    let name = telos_cli::session::next_session_name(dir.path(), "chat");
    assert!(name.starts_with("chat-"));
    assert!(name.ends_with(".json"));
}

#[test]
fn sessions_dir_with_project() {
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path();
    let sessions = telos_cli::session::sessions_dir(Some(project_root));
    assert!(sessions.starts_with(project_root));
    assert!(sessions.ends_with("sessions"));
}

#[test]
fn sessions_dir_without_project() {
    let sessions = telos_cli::session::sessions_dir(None);
    // Should be under the user's data directory
    assert!(sessions.to_string_lossy().contains("telos"));
    assert!(sessions.ends_with("sessions"));
}
