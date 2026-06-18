use assert_cmd::Command;
use predicates::prelude::*;

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
