# Tool Diagnostics and Optional GitHub Issue Reporting Design

## Context

The project already uses `tracing` macros in `core` and `cli`, but the CLI does not initialize a subscriber or provide a structured diagnostics pipeline. Tool failures are currently converted into `ToolResult { is_error: true }` inside `core/src/executor`, while Bash command failures include exit code, trimmed stdout, and trimmed stderr in the error payload returned to the model.

The new diagnostics feature must preserve tool execution failure information for local analysis and optionally report recurring sanitized failures to GitHub Issues for `future-re/telos-agent`. The CLI default behavior is local-only recording. GitHub reporting is opt-in only.

## Goals

- Add a project-level diagnostics dependency surface instead of ad hoc `eprintln!` or uninitialized `tracing` calls.
- Persist tool execution failures locally in a structured format.
- Capture all executor-level failure classes:
  - validation errors
  - permission denied / permission required / permission evaluation errors
  - tool not found
  - tool execution errors, including Bash non-zero exit codes and timeouts
  - tool panics
- Sanitize sensitive data before persistence and before any external reporting.
- Analyze failures locally and group recurring issues by stable sanitized signatures.
- Optionally create GitHub Issues in `future-re/telos-agent` only when explicitly enabled in config.
- Avoid coupling `telos_agent` core runtime to GitHub or any network client.

## Non-Goals

- Do not upload raw tool arguments, raw command strings, full paths, full stdout/stderr, environment values, API keys, model messages, or session transcripts.
- Do not create GitHub issues by default.
- Do not make every tracing log line part of the issue-reporting pipeline.
- Do not implement a remote telemetry service.
- Do not make the core crate depend on GitHub APIs, `reqwest`, or CLI configuration files.

## Architecture

### Core crate

Add a `diagnostics` module to `telos_agent` with small, dependency-light primitives:

- `ToolFailureEvent`
  - timestamp
  - session id
  - turn id
  - tool call id
  - tool name
  - failure kind
  - sanitized argument summary
  - sanitized error summary
  - optional exit code for Bash-style failures
  - stable sanitized signature
- `ToolDiagnosticsSink`
  - async trait used by the executor
  - receives sanitized `ToolFailureEvent`
- `NoopToolDiagnosticsSink`
  - default sink
- `JsonlToolDiagnosticsSink`
  - local JSONL sink for callers that want file persistence
- sanitization helpers
  - redacts secrets and token-like values
  - collapses absolute paths to placeholders
  - truncates output snippets
  - removes or masks emails, URLs with query strings, bearer tokens, API-key-like values, and configured environment values

Add an optional field to `AgentConfig`:

```rust
pub tool_diagnostics: Option<Arc<dyn ToolDiagnosticsSink>>;
```

The core default remains `None` so library users do not get file writes unless they opt in. The CLI should wire a local JSONL sink by default.

The executor records failures at the point where it already produces `ToolResult { is_error: true }`. This keeps the feature consistent across sync and streaming execution paths and avoids instrumenting each tool separately.

### CLI crate

The CLI owns configuration, directories, aggregation, and GitHub reporting:

- Parse a new config section:

```toml
[diagnostics]
enabled = true
retention_days = 14

[diagnostics.github]
enabled = false
repository = "future-re/telos-agent"
interval_hours = 24
min_occurrences = 3
```

- Resolve diagnostics directory:
  - project sessions: `<project_root>/.telos/diagnostics/`
  - no project root: platform data dir under `telos/diagnostics/`
- Wire `JsonlToolDiagnosticsSink` into `AgentConfig` when `[diagnostics].enabled = true`.
- Run local aggregation before or after a session, using the JSONL log as input.
- When GitHub reporting is disabled, write local issue drafts under `.telos/diagnostics/issues/`.
- When GitHub reporting is enabled and a GitHub token is available, create issues in `future-re/telos-agent`.

The GitHub token is read from `GITHUB_TOKEN` in the process environment or from `[env].GITHUB_TOKEN`. The token itself is never logged, stored in diagnostics, or included in issue content.

## Data Flow

1. A model calls a tool.
2. The executor validates arguments, checks permissions, and invokes the tool.
3. If the resulting `ToolResult` is an error, the executor builds a raw failure observation from already-available metadata.
4. The diagnostics module sanitizes the observation.
5. The configured sink writes a JSONL event locally.
6. The CLI aggregation pass groups events by sanitized signature.
7. If GitHub reporting is disabled, the CLI leaves local issue drafts only.
8. If GitHub reporting is enabled, enough occurrences exist, and no matching recent report exists, the CLI creates a GitHub Issue.

## Privacy Rules

Sanitization is mandatory before persistence and reporting. The implementation should treat local diagnostics as sensitive but still avoid storing raw secrets.

Redaction rules:

- Replace absolute paths under home, cwd, temp dirs, and project root with placeholders such as `<HOME>`, `<CWD>`, `<TMP>`, and `<PROJECT>`.
- Replace API-key-like strings with `<SECRET>`.
- Replace bearer tokens and authorization headers with `<SECRET>`.
- Replace configured environment values with `<ENV_VALUE>`.
- Replace email addresses with `<EMAIL>`.
- Strip URL query strings and fragments.
- Truncate stdout/stderr snippets to a small bounded size.
- Store Bash command summaries as command basename plus normalized structure where possible, not the full raw command.

GitHub issues must include only:

- tool name
- failure kind
- sanitized signature
- occurrence count
- first and last seen timestamps
- platform and telos version if available
- sanitized error summary
- sanitized command category for Bash failures

GitHub issues must not include:

- raw command strings
- raw arguments
- raw stdout/stderr
- absolute paths
- environment variable values
- API keys or tokens
- model messages
- user prompts
- session transcripts

## Failure Grouping

Each sanitized event has a stable signature derived from:

- tool name
- failure kind
- normalized error code or exit code
- sanitized command category for Bash
- normalized leading error line after redaction

Examples:

- `Bash:execution_error:exit=101:cargo:test:<redacted panic line hash>`
- `Read:validation_error:missing_required_field:file_path`
- `Edit:permission_denied:path_outside_workspace`

The aggregator stores reporting state locally so it does not create duplicate issues every run. A simple state file keyed by signature is enough for the first version.

## GitHub Reporting

Reporting is opt-in:

```toml
[diagnostics.github]
enabled = true
repository = "future-re/telos-agent"
interval_hours = 24
min_occurrences = 3
```

Behavior:

- If disabled, no network call is made.
- If enabled but no token is available, write a local issue draft and emit a warning through tracing.
- If enabled and token is available, create an issue through the GitHub REST API.
- Issue title format:
  - `Tool failure: <tool> <failure_kind> (<signature-short>)`
- Issue labels:
  - `tool-failure`
  - `automated`
  - `privacy-sanitized`

Before creating a new issue, the reporter checks local state for the signature and interval. The first version does not need to search GitHub for existing issues because local state prevents repeated reports from the same installation.

## Configuration Merge

User config and project config follow the existing merge model: project config overrides user config for matching fields. Diagnostics defaults:

- `[diagnostics].enabled = true` for CLI local recording.
- `[diagnostics.github].enabled = false`
- `repository = "future-re/telos-agent"`
- `interval_hours = 24`
- `min_occurrences = 3`
- `retention_days = 14`

CLI documentation should state clearly that GitHub reporting is opt-in and that diagnostics are sanitized before persistence.

## Testing

Core tests:

- Sanitizer redacts secrets, paths, emails, token-like strings, and URL query strings.
- Sanitizer truncates output snippets.
- Executor records validation failures.
- Executor records tool execution failures.
- Executor records panic failures.
- No sink configured means no diagnostics side effects.

CLI tests:

- Config parses `[diagnostics]` and `[diagnostics.github]`.
- Project config overrides user config.
- Diagnostics directory resolves under project `.telos/diagnostics`.
- Aggregator groups repeated sanitized failures.
- Reporter creates local drafts when GitHub is disabled or token is missing.

Network-facing GitHub tests should use a mock HTTP server or an injected reporter trait. No test should call the real GitHub API.

## Rollout

Implement in small steps:

1. Add core diagnostics data types, sanitizer, and sink trait.
2. Add JSONL sink and wire optional sink into `AgentConfig`.
3. Record executor failures.
4. Add CLI config and directory wiring.
5. Add local aggregation and issue draft generation.
6. Add opt-in GitHub reporter.
7. Document config and privacy behavior.

This sequence keeps local diagnostics useful before any network reporting exists and allows each step to be tested independently.
