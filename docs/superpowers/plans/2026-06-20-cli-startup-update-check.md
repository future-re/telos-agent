# CLI Startup Update Check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add startup update notices that compare the local `telos` version with crates.io and PyPI versions of `telos-cli`.

**Architecture:** Add a focused `cli/src/update_check.rs` module for registry fetching, cache handling, version comparison, and notice formatting. Wire it into `telos_cli::run()` after argument parsing while skipping completion, quiet mode, and `TELOS_DISABLE_UPDATE_CHECK=1`.

**Tech Stack:** Rust, reqwest, serde, semver, dirs, tempfile, existing Cargo test workflow.

---

### Task 1: Add Update-Check Unit Tests

**Files:**
- Create: `cli/src/update_check.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/Cargo.toml`

- [ ] **Step 1: Write failing tests**

Add tests to `cli/src/update_check.rs` for:

```rust
#[test]
fn notice_lists_each_registry_with_newer_version() {
    let status = UpdateStatus {
        current_version: "0.1.0".to_string(),
        crates_io: RegistryStatus::newer("0.2.0"),
        pypi: RegistryStatus::newer("0.3.0"),
    };

    let notice = format_update_notice(&status).expect("notice");

    assert!(notice.contains("telos 0.1.0 is not the latest version"));
    assert!(notice.contains("crates.io: 0.2.0"));
    assert!(notice.contains("cargo install --force telos-cli"));
    assert!(notice.contains("PyPI: 0.3.0"));
    assert!(notice.contains("pip install -U telos-cli"));
}

#[test]
fn notice_is_none_when_no_registry_has_newer_version() {
    let status = UpdateStatus {
        current_version: "0.2.0".to_string(),
        crates_io: RegistryStatus::current("0.2.0"),
        pypi: RegistryStatus::current("0.1.9"),
    };

    assert!(format_update_notice(&status).is_none());
}

#[test]
fn registry_failures_do_not_hide_other_updates() {
    let status = UpdateStatus {
        current_version: "0.1.0".to_string(),
        crates_io: RegistryStatus::Unavailable,
        pypi: RegistryStatus::newer("0.2.0"),
    };

    let notice = format_update_notice(&status).expect("notice");

    assert!(!notice.contains("crates.io:"));
    assert!(notice.contains("PyPI: 0.2.0"));
}
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test -p telos-cli update_check --lib`

Expected: compile failure because `UpdateStatus`, `RegistryStatus`, and `format_update_notice` do not exist.

- [ ] **Step 3: Implement minimal types and formatting**

Create `UpdateStatus`, `RegistryStatus`, and `format_update_notice`. Add `semver = "1"` to `cli/Cargo.toml`.

- [ ] **Step 4: Verify tests pass**

Run: `cargo test -p telos-cli update_check --lib`

Expected: all update-check unit tests pass.

### Task 2: Add Cache and Gating Tests

**Files:**
- Modify: `cli/src/update_check.rs`

- [ ] **Step 1: Write failing tests**

Add tests for:

```rust
#[test]
fn cache_is_fresh_for_less_than_twenty_four_hours() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(24 * 60 * 60);
    let checked_at = now - Duration::from_secs(23 * 60 * 60);

    assert!(is_cache_fresh(checked_at, now));
}

#[test]
fn cache_is_stale_at_twenty_four_hours() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(24 * 60 * 60);
    let checked_at = now - Duration::from_secs(24 * 60 * 60);

    assert!(!is_cache_fresh(checked_at, now));
}

#[test]
fn startup_check_is_disabled_by_quiet_flag() {
    assert!(!should_check_updates(true, None));
}

#[test]
fn startup_check_is_disabled_by_environment() {
    assert!(!should_check_updates(false, Some("1")));
    assert!(!should_check_updates(false, Some("true")));
}
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test -p telos-cli update_check --lib`

Expected: compile failure because `is_cache_fresh` and `should_check_updates` do not exist.

- [ ] **Step 3: Implement cache freshness and gating**

Add `is_cache_fresh` with a 24-hour threshold and `should_check_updates` with quiet/env suppression.

- [ ] **Step 4: Verify tests pass**

Run: `cargo test -p telos-cli update_check --lib`

Expected: all update-check unit tests pass.

### Task 3: Add Registry Fetching and Startup Wiring

**Files:**
- Modify: `cli/src/update_check.rs`
- Modify: `cli/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Add serde parsing tests for crates.io search payload and PyPI info payload:

```rust
#[test]
fn parses_crates_io_max_version() {
    let body = r#"{"crates":[{"id":"telos-cli","max_version":"0.2.0"}]}"#;

    assert_eq!(parse_crates_io_latest(body).unwrap(), "0.2.0");
}

#[test]
fn parses_pypi_info_version() {
    let body = r#"{"info":{"version":"0.3.0"}}"#;

    assert_eq!(parse_pypi_latest(body).unwrap(), "0.3.0");
}
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test -p telos-cli update_check --lib`

Expected: compile failure because parsing functions do not exist.

- [ ] **Step 3: Implement fetching and startup call**

Implement `maybe_print_update_notice(current_version, quiet)` and call it from `run()` after parsing CLI args unless the command is `Completion`.

- [ ] **Step 4: Verify update-check tests pass**

Run: `cargo test -p telos-cli update_check --lib`

Expected: update-check unit tests pass.

### Task 4: Full Verification

**Files:**
- All changed Rust files.

- [ ] **Step 1: Format**

Run: `cargo fmt --all -- --check`

Expected: no formatting diffs.

- [ ] **Step 2: Run CLI tests**

Run: `cargo test -p telos-cli`

Expected: all `telos-cli` unit and integration tests pass.

- [ ] **Step 3: Run workspace tests**

Run: `cargo test --workspace`

Expected: all workspace tests pass.
