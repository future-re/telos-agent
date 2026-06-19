# TUI Overlay + Command Completion Status

**Goal:** Bring the TUI in line with the Codex-style design: trait-based overlays, richer chat cells, real slash-command behavior, resumable sessions, model switching, and focused regression tests.

**Current status:** Implemented as an incremental TUI enhancement. The core runtime remains unchanged; the TUI owns presentation state and sends control commands to the background session task.

## Implemented

- Overlay stack with `Overlay`, `ApprovalOverlay`, `SelectionPopup`, and `UserInputPopup`.
- Approval popup supports approve, deny, edit-as-JSON, and a current-process remember toggle indicator.
- Chat history uses polymorphic cells for user, assistant, thinking, tool, separator, and error entries.
- Assistant and thinking streams are tracked separately so deltas do not merge into the wrong cell.
- Tool cells retain progress when marked completed and shell tools can be selected and expanded.
- `/model` switches the provider for later turns in the current TUI process when a DeepSeek API key is available.
- `/session` supports new session, list sessions, and resume from JSONL storage.
- `/tool` shows registered tools, aliases, and descriptions.
- `Ctrl+C` cancellation is reset before the next prompt.

## Design Notes

- Model switching is intentionally session-local. It does not write `~/.config/telos/config.toml`.
- Session management uses the existing `JsonlStorage` directory; it does not rename or delete sessions.
- Tool management is read-only in the TUI. Enabling/disabling tools and editing policies remain out of scope.
- `ToolCallCell` covers command execution display, so there is no separate `CommandCell` type.
- The background session task receives commands instead of raw prompt strings: prompt, provider switch, new session, and resume session.

## Verification

- Unit tests cover chat-cell stream separation, streaming completion, tool expansion/progress retention, approval edit triggering, popup truncation, auto-approval safety, and cancellation reset.
- Full regression command: `cargo test`.

## Follow-Ups

- Persist model choices if product direction changes from session-local to config-backed.
- Add richer session management actions such as delete and rename.
- Add visual snapshot tests for overlay rendering when the TUI snapshot harness exists.
