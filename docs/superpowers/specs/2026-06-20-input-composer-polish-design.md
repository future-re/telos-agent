# Input Composer Polish Design

## Overview
Polish the TUI user input panel so it reads as a focused composer rather than a plain bordered text box. The chosen direction is the Codex-style composer from the visual companion: clearer focus, shorter guidance, stronger hierarchy, and no behavior changes.

## Scope
- Update `cli/src/tui/input_panel.rs` rendering and copy for the normal input panel.
- Add minimal theme fields in `cli/src/tui/theme.rs` only if they prevent scattered hard-coded colors.
- Preserve current input behavior: submit on Enter, newline on Alt+Enter, slash command detection, history navigation, paste confirmation, inactive streaming state, and command popup positioning.
- Keep `UserInputPopup` unchanged unless the implementation exposes a shared style issue that directly affects the composer.

## Visual Design
The active panel title becomes `Compose`, with a compact right-side hint such as `/ commands`. The prompt marker changes from `> ` to a lighter `›`, styled with the user/accent color. Placeholder copy becomes shorter and task-oriented, for example `Ask tiny-agent to edit, inspect, or run...`.

The footer hint is split into two logical groups instead of one long dense string: primary actions on the left and secondary navigation/quit hints on the right when width allows. On narrow widths it can degrade to a single concise line without wrapping into the input area.

Paste confirmation and history browsing remain visually distinct. Paste mode keeps an explicit title such as `Pasted N lines`, while history mode keeps its counter but follows the new footer styling.

## Architecture
`InputPanel::render` remains the single owner of composer layout. The change should avoid introducing a new widget unless the function becomes materially harder to read. Theme additions should be small and semantic, for example composer accent and muted colors, and should reuse existing colors where possible.

## Testing
Add focused unit-level coverage only if the changed logic can be factored without overbuilding. Otherwise verify with existing Rust tests and a build/test run. The key regression risks are layout under small terminal widths and preserving all keyboard behavior.

## Self-Review
- [x] No placeholders or unresolved TODOs
- [x] Scope is limited to input composer visual polish
- [x] Existing behavior is explicitly preserved
- [x] Testing expectations are concrete for a TUI-only visual change
