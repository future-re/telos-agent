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
