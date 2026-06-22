# Python Textual TUI Implementation Plan

**Goal:** Ship a first working Python Textual TUI that talks to `telos serve` over JSONL, while leaving the Rust CLI and runtime behavior intact.

**Architecture:** A Python package under `cli/python/telos_tui/` owns UI state, rendering, input, and subprocess IO. The Rust backend remains the source of truth for agent execution, tools, approvals, config, and session lifecycle.

**Tech Stack:** Python 3.8+, Textual, stdlib `asyncio`/`subprocess`, existing `telos serve` Rust backend, pytest for Python-side tests.

## Constraints

- Keep the existing `telos` Rust command working unchanged.
- Do not reimplement runtime, provider, tools, memory, or approval policy in Python.
- Use `telos serve` as the only backend protocol for milestone one.
- Keep Python code isolated under `cli/python/` so root `maturin` packaging is not destabilized early.
- Treat unknown backend events defensively; never crash the UI on protocol drift.

## File Map

| File | Responsibility |
|---|---|
| `cli/python/telos_tui/__init__.py` | package marker and version stub |
| `cli/python/telos_tui/__main__.py` | `python -m telos_tui` entrypoint |
| `cli/python/telos_tui/app.py` | `Textual` app root and global keybindings |
| `cli/python/telos_tui/backend.py` | async `telos serve` subprocess client |
| `cli/python/telos_tui/protocol.py` | command/event models and event mapping |
| `cli/python/telos_tui/transcript.py` | pure state model for transcript cells |
| `cli/python/telos_tui/widgets/transcript.py` | transcript viewport widgets |
| `cli/python/telos_tui/widgets/prompt.py` | prompt composer widget |
| `cli/python/telos_tui/widgets/tool_call.py` | tool row rendering |
| `cli/python/telos_tui/widgets/approval.py` | approval modal widget |
| `cli/python/telos_tui/widgets/status_bar.py` | compact status bar |
| `cli/python/telos_tui/styles.tcss` | first-pass layout and palette |
| `cli/tests/python_tui/test_transcript.py` | transcript state tests |
| `cli/tests/python_tui/test_protocol.py` | event mapping tests |
| `cli/tests/python_tui/test_backend.py` | fake-backend subprocess tests |
| `cli/tests/python_tui/test_smoke.py` | minimal TUI smoke path with fake backend |
| `cli/docs/superpowers/specs/2026-06-22-python-textual-tui-design.md` | approved design |
| `cli/docs/superpowers/plans/2026-06-22-python-textual-tui-implementation-plan.md` | this plan |

## Phase 1: Package Skeleton And Local Run Path

### Task 1: Create Python package skeleton

**Files:**
- Create: `cli/python/telos_tui/__init__.py`
- Create: `cli/python/telos_tui/__main__.py`
- Create: `cli/python/telos_tui/widgets/__init__.py`

- [ ] Add a minimal package layout that supports `PYTHONPATH=python python -m telos_tui`.
- [ ] Keep package metadata out of this phase; rely on `PYTHONPATH` for local iteration first.
- [ ] Make `__main__.py` call an async `run()` owned by `app.py`.

**Verification:**

```bash
cd cli
PYTHONPATH=python python -m telos_tui --help
```

Expected: entrypoint runs and prints a minimal usage or startup message.

## Phase 2: Protocol And State Core

### Task 2: Implement protocol mapping

**Files:**
- Create: `cli/python/telos_tui/protocol.py`

- [ ] Define typed helpers for frontend commands: `run`, `new_session`, `_approve`, `quit`.
- [ ] Define an event parser that accepts raw JSON lines and returns normalized event objects or diagnostic events.
- [ ] Cover the currently emitted backend events from [`src/serve.rs`](/home/alin/codework/tiny_agent/tiny_agent_core/cli/src/serve.rs).
- [ ] Preserve unknown event payloads for compact diagnostic rendering.

**Verification:**

```bash
cd cli
PYTHONPATH=python pytest tests/python_tui/test_protocol.py
```

### Task 3: Implement transcript state model

**Files:**
- Create: `cli/python/telos_tui/transcript.py`

- [ ] Model append-only cells for user, assistant, thinking, tool call, tool result, approval, separator, and error rows.
- [ ] Support streaming assistant and thinking deltas into the active cell.
- [ ] Upsert tool state by tool ID for progress, completion, and result preview.
- [ ] Expose reset/finalize operations for `_done` and `_session_new`.
- [ ] Keep this module UI-agnostic and unit-testable.

**Verification:**

```bash
cd cli
PYTHONPATH=python pytest tests/python_tui/test_transcript.py
```

## Phase 3: Backend Client

### Task 4: Build async `telos serve` client

**Files:**
- Create: `cli/python/telos_tui/backend.py`

- [ ] Spawn `telos serve` with inherited env and forwarded CLI options where needed.
- [ ] Read stdout as JSONL events and stderr as status/error text.
- [ ] Provide async methods for `send_run(prompt)`, `send_new_session()`, `send_approve(decision)`, and `shutdown()`.
- [ ] Surface backend exit and stdin write failures as explicit frontend errors.
- [ ] Avoid blocking Textual's event loop; IO must run in background tasks.

**Verification:**

```bash
cd cli
PYTHONPATH=python pytest tests/python_tui/test_backend.py
```

## Phase 4: First Textual UI

### Task 5: Build the root app shell

**Files:**
- Create: `cli/python/telos_tui/app.py`
- Create: `cli/python/telos_tui/styles.tcss`

- [ ] Compose three regions: transcript, prompt composer, status bar.
- [ ] Start backend client on mount and stop it on exit.
- [ ] Bind first-pass keys: `enter`, `ctrl+d`, `ctrl+l`, `up`, `down`, `y`, `n`, `escape`.
- [ ] Block prompt submission while approval is pending.
- [ ] Render a clear persistent error state when backend startup fails.

### Task 6: Build transcript and tool widgets

**Files:**
- Create: `cli/python/telos_tui/widgets/transcript.py`
- Create: `cli/python/telos_tui/widgets/tool_call.py`
- Create: `cli/python/telos_tui/widgets/status_bar.py`

- [ ] Render user prompts with a simple leading marker.
- [ ] Render assistant and thinking output as separate visual states.
- [ ] Render tool rows with status marker, summary line, expandable details, and result preview.
- [ ] Keep the first version non-virtualized unless transcript performance becomes a real problem.

### Task 7: Build prompt and approval widgets

**Files:**
- Create: `cli/python/telos_tui/widgets/prompt.py`
- Create: `cli/python/telos_tui/widgets/approval.py`

- [ ] Implement multiline input with send-on-enter for non-empty input.
- [ ] Preserve local prompt history within the current app session.
- [ ] Show approval details in a focused modal and send `_approve` decisions back to the backend.
- [ ] Default `escape` to deny and close the modal.

**Verification:**

```bash
cd cli
PYTHONPATH=python pytest tests/python_tui/test_smoke.py
```

## Phase 5: Integration And CLI Fit

### Task 8: Decide startup wiring without breaking Rust defaults

**Files:**
- Optional later change: `cli/src/cli.rs`
- Optional later change: `cli/src/main.rs`
- Optional later change: root `pyproject.toml`

- [ ] Keep milestone one launched manually through `python -m telos_tui`.
- [ ] Do not switch the default `telos` interactive path in this phase.
- [ ] After the Python TUI is stable, evaluate adding a Rust subcommand that shells out to Python, or a packaged console script such as `telos-py`.

## Test Strategy

### Python-side automated tests

- [ ] `test_transcript.py`: assistant/thinking streaming merge, tool lifecycle upsert, done/reset handling.
- [ ] `test_protocol.py`: parse known backend events and preserve unknown events as diagnostics.
- [ ] `test_backend.py`: fake subprocess emits representative JSONL; verify commands written and events consumed.
- [ ] `test_smoke.py`: mount app with a fake backend and verify prompt submit, transcript update, approval modal, and turn completion.

### Rust-side confidence checks

- [ ] Re-run `cargo test -p telos-cli` after any change touching `src/serve.rs` or CLI wiring.
- [ ] Add a targeted Rust test only if the serve protocol itself changes.

## Manual Verification

- [ ] Start a local session with mock provider:

```bash
cd cli
PYTHONPATH=python python -m telos_tui --provider mock
```

- [ ] Submit a prompt and confirm user row, streaming assistant row, and done state.
- [ ] Run against a fake backend fixture that emits tool call, progress, result, and approval events.
- [ ] Verify `ctrl+l` clears transcript only on the frontend.
- [ ] Verify `new_session` clears active state and waits for new turns cleanly.
- [ ] Verify backend crash or malformed JSON produces visible error rows instead of a dead UI.

## Deferred Work

- Transcript virtualization for long sessions.
- Richer Markdown rendering parity with the Rust TUI.
- Cancellation semantics if backend support is added.
- Packaging the Python UI into the root distribution.
- Making the Python TUI the default interactive frontend.

## Exit Criteria

- `PYTHONPATH=python python -m telos_tui` launches a usable full-screen Textual app.
- The app can send prompts to `telos serve` and render streamed output, tools, approvals, and errors.
- Core Python tests for protocol, transcript, backend, and smoke flow pass locally.
- No default Rust CLI behavior regresses.
