# Python Textual TUI Design

## Decision

Build a new Python TUI frontend using Textual. The Python app will own the terminal interface and communicate with the existing Rust backend through `telos serve` JSON lines.

This preserves the current `telos_agent` runtime, provider configuration, tools, memory, diagnostics, and approval policies while allowing the terminal UI to be rewritten from scratch in Python.

## Goals

- Recreate the core `learn-claude-code` REPL experience in Python:
  - full-screen transcript
  - bottom prompt composer
  - streaming assistant output
  - thinking output
  - tool-call rows with progress and result previews
  - approval modal
  - status bar
  - keyboard-first navigation
- Keep the first milestone focused on the interactive agent loop.
- Avoid rewriting agent/provider/tool/runtime behavior in Python.
- Keep the current Rust CLI behavior available while the Python TUI is introduced.

## Non-Goals

- Reimplementing the model provider layer in Python.
- Reimplementing tools, memory, diagnostics, onboarding, or billing in Python.
- Matching every advanced `learn-claude-code` feature in the first milestone, including voice mode, IDE integration, plugin management, background agents, complex search, or remote sessions.
- Replacing `telos serve` with a new protocol before the first Python UI works.

## Architecture

The Python package will provide an executable entrypoint such as `telos-py` or `python -m telos_tui`.

At startup, the Python app will spawn the current `telos serve` command as a subprocess. Commands are written to the subprocess stdin as one JSON object per line. Backend events are read from stdout as JSON lines and mapped into Python UI state.

The backend remains responsible for:

- configuration and provider resolution
- `AgentSession`
- tool registry and tool execution
- approval policy evaluation
- memory and diagnostics
- session reset through `new_session`

The Python frontend is responsible for:

- terminal layout
- transcript state
- keyboard handling
- input editing and history
- rendering streamed output
- rendering tool state
- approval responses
- status and error presentation

## Protocol Mapping

Python sends:

- `{"cmd":"run","prompt":"..."}`
- `{"cmd":"new_session"}`
- `{"cmd":"_approve","decision":"allow"}`
- `{"cmd":"_approve","decision":"deny"}`
- `{"cmd":"quit"}`

Python receives and maps:

- `AssistantDelta` -> append text to the active assistant streaming cell
- `ThinkingDelta` -> append text to the active thinking cell
- `ToolCall` -> create a tool-call cell
- `ToolProgress` -> append progress to the matching tool-call cell
- `ToolCompleted` -> mark the matching tool-call cell succeeded or failed
- `ToolResult` -> attach result preview lines to the matching tool-call cell
- `_approval_required` -> open approval modal and block prompt submission until resolved
- `_done` -> mark turn complete
- `_error` -> append an error cell and mark turn complete
- `_session_new` -> clear active turn state and optionally add a separator

If a backend event is unknown, the TUI will show a compact diagnostic row instead of crashing.

## Components

### `TelosTuiApp`

Textual application root. It owns subprocess lifecycle, background tasks for JSONL IO, global keybindings, and high-level turn state.

### `BackendClient`

Async subprocess wrapper for `telos serve`.

Responsibilities:

- spawn the backend with the current CLI options
- write JSONL commands
- read JSONL events
- surface backend stderr as status/error messages
- terminate the subprocess on app exit

### `TranscriptStore`

Pure Python state model for the conversation. It stores cells in append order and provides operations such as:

- append user prompt
- append assistant delta
- append thinking delta
- upsert tool progress/result by tool ID
- finish active streaming cells
- clear transcript

This state is unit-testable without Textual.

### `TranscriptView`

Scrollable message viewport inspired by `learn-claude-code`'s `VirtualMessageList`. The first version can use Textual's normal scroll container; virtualization can be added later if transcript size becomes a problem.

### `PromptInput`

Bottom composer with multiline editing.

Required first-version behavior:

- Enter sends when input is non-empty
- Alt+Enter inserts newline if Textual exposes the key distinctly in the target terminals
- Ctrl+D exits when input is empty
- Ctrl+C cancels or marks the active turn interrupted if backend cancellation is supported later
- Ctrl+L clears transcript
- Up/Down navigate prompt history when appropriate

### `ToolCallView`

Renders tool state as compact rows:

- pending/running/completed/failed marker
- tool name and detail
- progress lines
- collapsed result preview
- expand/collapse support for selected tool rows

Shell-like tool rows default to collapsed previews. Non-shell tool rows default to expanded.

### `ApprovalModal`

Displays `_approval_required` details. Keybindings:

- `y` or `a` -> allow
- `n` or `d` -> deny
- `esc` -> deny

The modal sends `_approve` to the backend and then closes.

### `StatusBar`

Shows provider/model/cwd when available, turn state, auto mode indicator if available, and transient backend errors.

## Visual Direction

The layout follows the Claude Code style rather than the current Rust ratatui structure:

- transcript fills most of the screen
- prompt composer is visually anchored at the bottom
- status line stays compact
- user messages use a `>` or `❯`-style marker
- thinking output is subdued
- tool calls use terse single-line summaries with expandable detail
- approval appears as a focused modal over the transcript

The palette should stay restrained and terminal-native: cyan/green for active input, muted gray for thinking, yellow for running tools, green/red for tool completion.

## Error Handling

- Invalid backend JSON lines become error cells with the raw line truncated.
- Backend process exit becomes a persistent error state and disables prompt submission.
- Failed writes to backend stdin become error cells and mark the turn inactive.
- Unknown event types become diagnostic cells.
- Approval responses with no pending request are ignored by the backend; the UI should also clear stale modal state.

## Testing

Use test-first implementation for behavior changes.

Focused tests:

- `TranscriptStore` appends and merges streaming assistant deltas.
- `TranscriptStore` creates and updates tool-call cells by ID.
- Backend event mapper handles all currently emitted `telos serve` event types.
- Approval modal decisions serialize the expected `_approve` commands.
- `BackendClient` can be tested against a fake subprocess or stream pair without requiring a real model call.
- A smoke test can run the TUI against a fake backend script that emits representative JSONL events.

Manual verification:

- launch Python TUI with mock provider
- send a prompt
- observe streaming assistant text
- observe a tool-call fixture or fake backend event sequence
- approve/deny modal behavior
- clear transcript and new-session commands

## Open Implementation Choices

- Package location: prefer a Python package under `cli/python/telos_tui/` or `python/telos_tui/`, depending on packaging constraints discovered during implementation.
- Public command name: prefer `telos-py` until the Python TUI is ready to become the default `telos` interactive mode.
- Textual dependency declaration: add it to the Python packaging metadata once the final package layout is selected.

These choices do not change the core architecture.
