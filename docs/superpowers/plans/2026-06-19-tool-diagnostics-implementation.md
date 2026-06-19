# Tool Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured local tool-failure diagnostics with sanitized JSONL persistence and opt-in GitHub Issue reporting for recurring failures.

**Architecture:** `telos_agent` owns generic diagnostics types, sanitization, sink traits, and executor instrumentation. `telos-cli` owns config parsing, diagnostics directory wiring, local aggregation, issue draft generation, and the GitHub HTTP reporter. The core runtime never depends on GitHub or network clients.

**Tech Stack:** Rust 2024, Tokio, serde/serde_json, async-trait, existing executor and config layers, `reqwest` for CLI-only GitHub REST calls, `wiremock` for reporter tests.

---

## File Structure

- Create `core/src/diagnostics.rs`: tool failure event model, sanitizer, sink trait, noop sink, JSONL sink, signature generation.
- Modify `core/src/config.rs`: add optional diagnostics sink to `AgentConfig`.
- Modify `core/src/lib.rs`: export diagnostics types.
- Modify `core/src/executor/invoke.rs`: record validation, permission, execution, timeout, and tool result failures through the sink.
- Modify `core/src/executor/sync.rs`: record panic and tool-not-found failures.
- Modify `core/src/executor/stream.rs`: record panic and tool-not-found failures in streaming execution.
- Modify `core/Cargo.toml`: add `async-trait` use is already available; no new core dependency needed.
- Create `cli/src/diagnostics.rs`: config-to-core wiring, diagnostics directory resolution, aggregator, draft writer, GitHub reporter.
- Modify `cli/src/config.rs`: parse and merge `[diagnostics]` and `[diagnostics.github]`.
- Modify `cli/src/lib.rs`: wire diagnostics sink into every `AgentConfig` created by the CLI and trigger aggregation/reporting.
- Modify `cli/src/runner.rs` and TUI path only if needed to pass project root/config-derived diagnostics through existing config construction.
- Modify `cli/Cargo.toml`: add `reqwest` and `wiremock` dev dependency if reporter tests need HTTP mocking.
- Modify `README.md` and `cli/README.md`: document diagnostics defaults, privacy, and opt-in GitHub reporting.

---

## Task 1: Core Diagnostics Primitives

**Files:**
- Create: `core/src/diagnostics.rs`
- Modify: `core/src/lib.rs`
- Test: `core/src/diagnostics.rs`

- [ ] **Step 1: Write failing sanitizer and sink tests**

Add tests in `core/src/diagnostics.rs` for:

```rust
#[test]
fn sanitizer_redacts_sensitive_values() {
    let mut sanitizer = ToolFailureSanitizer::new(
        PathBuf::from("/home/alice/project"),
        HashMap::from([("API_KEY".into(), "sk-secret-value".into())]),
    );
    sanitizer.home_dir = Some(PathBuf::from("/home/alice"));
    let text = "token sk-secret-value at /home/alice/project/src/main.rs user a@example.com https://example.com/path?secret=1";
    let sanitized = sanitizer.sanitize_text(text);
    assert!(!sanitized.contains("sk-secret-value"));
    assert!(!sanitized.contains("/home/alice"));
    assert!(!sanitized.contains("a@example.com"));
    assert!(!sanitized.contains("secret=1"));
    assert!(sanitized.contains("<ENV_VALUE>"));
    assert!(sanitized.contains("<PROJECT>"));
    assert!(sanitized.contains("<EMAIL>"));
}

#[test]
fn event_signature_is_stable_for_sanitized_failures() {
    let event = ToolFailureEvent::new(
        "s1",
        2,
        "call-1",
        "Bash",
        ToolFailureKind::ExecutionError,
        SanitizedToolFailure {
            argument_summary: "cargo test".into(),
            error_summary: "Command failed with exit code Some(101)".into(),
            exit_code: Some(101),
        },
    );
    assert_eq!(event.signature, "Bash:execution_error:exit=101:cargo test:Command failed with exit code Some(101)");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p telos_agent diagnostics::tests::sanitizer_redacts_sensitive_values diagnostics::tests::event_signature_is_stable_for_sanitized_failures`

Expected: FAIL because `diagnostics` module and types do not exist.

- [ ] **Step 3: Implement minimal diagnostics module**

Implement `ToolFailureKind`, `SanitizedToolFailure`, `ToolFailureEvent`, `ToolFailureSanitizer`, `ToolDiagnosticsSink`, `NoopToolDiagnosticsSink`, and `JsonlToolDiagnosticsSink`.

- [ ] **Step 4: Export diagnostics module**

Add `pub mod diagnostics;` and public re-exports in `core/src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p telos_agent diagnostics`

Expected: PASS.

---

## Task 2: Wire Core Executor Recording

**Files:**
- Modify: `core/src/config.rs`
- Modify: `core/src/executor/invoke.rs`
- Modify: `core/src/executor/sync.rs`
- Modify: `core/src/executor/stream.rs`
- Test: `core/src/executor/tests.rs` or local executor tests

- [ ] **Step 1: Write failing executor diagnostics tests**

Add tests that configure an in-memory diagnostics sink and assert:

```rust
#[tokio::test]
async fn executor_records_tool_execution_failure() {
    let sink = Arc::new(MemoryDiagnosticsSink::default());
    let config = AgentConfig { tool_diagnostics: Some(sink.clone()), ..AgentConfig::default() };
    let mut tools = ToolRegistry::new();
    tools.register(FailingTool);
    let calls = vec![ToolCall {
        id: "call-1".into(),
        name: "Fail".into(),
        arguments: serde_json::json!({}),
    }];
    let output = execute_tool_calls(
        calls,
        &tools,
        &config,
        "session-1",
        1,
        Arc::new(vec![]),
        Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    ).await;
    assert!(output.results[0].is_error);
    let events = sink.events.lock().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tool_name, "Fail");
    assert_eq!(events[0].failure_kind, ToolFailureKind::ExecutionError);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p telos_agent executor_records_tool_execution_failure`

Expected: FAIL because `AgentConfig::tool_diagnostics` does not exist and executor does not record failures.

- [ ] **Step 3: Implement executor recording**

Add `tool_diagnostics` to `AgentConfig`, default it to `None`, and call a helper whenever a failure `ToolResult` is created. For panics and tool-not-found paths, record in `sync.rs` and `stream.rs` where those results are constructed.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p telos_agent executor_records_tool_execution_failure diagnostics`

Expected: PASS.

---

## Task 3: CLI Config and Wiring

**Files:**
- Create: `cli/src/diagnostics.rs`
- Modify: `cli/src/config.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/src/runner.rs` if required by current control flow
- Test: `cli/src/config.rs`, `cli/src/diagnostics.rs`

- [ ] **Step 1: Write failing config tests**

Add tests in `cli/src/config.rs`:

```rust
#[test]
fn parses_diagnostics_config() {
    let cfg: FileConfig = toml::from_str(r#"
[diagnostics]
enabled = true
retention_days = 7

[diagnostics.github]
enabled = true
repository = "future-re/telos-agent"
interval_hours = 12
min_occurrences = 2
"#).unwrap();
    let diagnostics = cfg.diagnostics.unwrap();
    assert_eq!(diagnostics.enabled, Some(true));
    assert_eq!(diagnostics.retention_days, Some(7));
    let github = diagnostics.github.unwrap();
    assert_eq!(github.enabled, Some(true));
    assert_eq!(github.repository.as_deref(), Some("future-re/telos-agent"));
    assert_eq!(github.interval_hours, Some(12));
    assert_eq!(github.min_occurrences, Some(2));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p telos-cli parses_diagnostics_config`

Expected: FAIL because config structs do not include diagnostics.

- [ ] **Step 3: Implement CLI config parsing and merging**

Add `DiagnosticsSection` and `DiagnosticsGithubSection` to `FileConfig`, merge user/project sections field-by-field, and default CLI local diagnostics to enabled in wiring.

- [ ] **Step 4: Implement diagnostics sink wiring**

Add `cli/src/diagnostics.rs` helpers:

```rust
pub fn diagnostics_dir(project_root: Option<&Path>) -> anyhow::Result<PathBuf>;
pub fn configure_tool_diagnostics(config: &mut AgentConfig, file_config: &FileConfig, project_root: Option<&Path>) -> anyhow::Result<Option<DiagnosticsRuntime>>;
```

Wire this after every `build_agent_config` call before constructing `AgentSession`.

- [ ] **Step 5: Run focused CLI tests**

Run: `cargo test -p telos-cli diagnostics config::tests::parses_diagnostics_config`

Expected: PASS.

---

## Task 4: Local Aggregation and Drafts

**Files:**
- Modify: `cli/src/diagnostics.rs`
- Test: `cli/src/diagnostics.rs`

- [ ] **Step 1: Write failing aggregation tests**

Add tests that write two sanitized JSONL failures with the same signature, aggregate them, and assert one issue draft is generated when GitHub is disabled.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p telos-cli aggregates_repeated_failures creates_issue_draft_when_github_disabled`

Expected: FAIL because aggregation and draft writing do not exist.

- [ ] **Step 3: Implement aggregation and draft writer**

Implement grouping by signature, `min_occurrences`, local reporting state, and Markdown drafts under `<diagnostics_dir>/issues/`.

- [ ] **Step 4: Run focused aggregation tests**

Run: `cargo test -p telos-cli diagnostics`

Expected: PASS.

---

## Task 5: Opt-In GitHub Reporter

**Files:**
- Modify: `cli/Cargo.toml`
- Modify: `cli/src/diagnostics.rs`
- Test: `cli/src/diagnostics.rs`

- [ ] **Step 1: Write failing reporter tests**

Add a mock-HTTP test that posts to `/repos/future-re/telos-agent/issues` only when GitHub reporting is enabled and a token exists.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p telos-cli github_reporter_posts_sanitized_issue`

Expected: FAIL because reporter does not exist.

- [ ] **Step 3: Add CLI HTTP dependencies**

Add `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }` and `wiremock = "0.6"` as a CLI dev dependency.

- [ ] **Step 4: Implement reporter**

Implement a GitHub reporter that accepts a base URL for tests, sends sanitized issue title/body/labels, and does nothing unless `[diagnostics.github].enabled = true` and a token is available.

- [ ] **Step 5: Run reporter tests**

Run: `cargo test -p telos-cli github_reporter_posts_sanitized_issue`

Expected: PASS.

---

## Task 6: Documentation and Workspace Verification

**Files:**
- Modify: `README.md`
- Modify: `cli/README.md`
- Test: workspace commands

- [ ] **Step 1: Document diagnostics behavior**

Add config examples showing local diagnostics enabled by default and GitHub reporting disabled by default. Include privacy guarantees and opt-in token requirements.

- [ ] **Step 2: Run full tests**

Run: `cargo test --workspace`

Expected: PASS.

- [ ] **Step 3: Run formatting**

Run: `cargo fmt --all -- --check`

Expected: PASS.

- [ ] **Step 4: Commit implementation**

Commit only diagnostics-related files and leave unrelated existing changes untouched.
