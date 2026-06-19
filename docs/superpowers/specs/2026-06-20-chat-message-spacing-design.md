# Chat Message Spacing Design

## Overview
Improve the TUI chat transcript density so user prompts and assistant replies are easier to scan. The current rendering feels too tight because both roles read as plain text in the same flow. The change should create clearer visual hierarchy without adding heavy boxes or changing conversation behavior.

## Scope
- Adjust message rendering in `cli/src/tui/history_cell.rs`.
- Add or update focused `cli/src/tui/chat_widget.rs` tests if needed to lock the intended spacing.
- Preserve the existing bottom-aligned chat behavior from the previous TUI change.
- Do not change input handling, status bar, tool activity, provider/core logic, or session persistence.

## Visual Design
User messages should render as a compact prompt block with a blank separator above, a `▸` marker, and indented continuation lines. This keeps the user turn visually distinct from assistant text without creating a heavy card.

Assistant messages should gain light breathing room after a user message and keep readable markdown output. The assistant block should remain quiet: no border, no card, no large heading, and no aggressive color. The goal is a calmer transcript, not a decorative chat UI.

Role separation is handled through whitespace and indentation:
- User block: one separator line above, accent marker, body aligned after the marker.
- Assistant block: one separator line above when needed, then normal markdown rendering.
- Long wrapped lines should not collide with role markers.

## Architecture
Keep spacing responsibility inside `HistoryCell` implementations. `ChatWidget` should continue treating cells as measured/rendered blocks with `needed_lines`, `render`, and `render_scrolled`. Any new helper should be private to `history_cell.rs` unless tests need access.

## Testing
Use `ratatui::backend::TestBackend` tests to verify rendered rows for short transcripts. Cover at least one user prompt and one assistant reply so regressions in spacing are caught without fragile full-screen snapshots.

## Self-Review
- [x] Scope is limited to TUI chat message rendering
- [x] Existing bottom alignment remains in scope as a preserved behavior
- [x] No core/provider/session behavior changes
- [x] Testing approach is concrete and local to TUI rendering
