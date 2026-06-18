use assert_cmd::Command;
use predicates::prelude::*;

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
