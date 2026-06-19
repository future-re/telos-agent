# TUI Enhancement Design

## Overview
Enhance the tiny-agent TUI by adopting Codex CLI's architectural patterns: polymorphic HistoryCell trait, popup/overlay system, enhanced Chat Composer, status indicator, and diff rendering.

## Phase 1: HistoryCell + Chat Composer

### HistoryCell Trait
Replace the flat `UiMessage` enum with a trait-based cell system:

```rust
pub trait HistoryCell: Send {
    fn needed_lines(&self, width: usize) -> u16;
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme);
    fn is_streaming(&self) -> bool { false }
    fn push_text(&mut self, _text: &str) {}
}
```

Cell implementations:
- `UserCell` - user input with `▸` prefix, bold styling
- `AgentCell` - assistant response, markdown-rendered, supports streaming by accumulating text into a buffer and re-rendering markdown on flush
- `ThinkingCell` - reasoning content, dimmed/italic style
- `ToolCallCell` - tool invocation with pending/running/completed states
- `CommandCell` - shell command execution display
- `SeparatorCell` - turn separator line
- `ErrorCell` - error messages with red styling

`ChatWidget` will own `Vec<Box<dyn HistoryCell>>` instead of `Vec<UiMessage>`.

### Chat Composer
Input state machine: `Normal | SlashCommand | Pasting`
- Slash command detection: `/` at empty buffer opens completion popup
- Built-in commands: `/tool`, `/model`, `/help`, `/clear`, `/session`, `/auto`
- Paste detection: >3 lines triggers confirmation prompt
- Command popup widget for autocomplete suggestions

### Files Changed
| Action | Path | Description |
|--------|------|-------------|
| NEW | `cli/src/tui/history_cell.rs` | HistoryCell trait + all implementations |
| NEW | `cli/src/tui/chat_widget.rs` | New ChatWidget (replaces ChatPanel) |
| NEW | `cli/src/tui/command_popup.rs` | Slash command autocomplete popup |
| REWRITE | `cli/src/tui/input_panel.rs` | Input state machine + slash detection |
| MODIFY | `cli/src/tui/app.rs` | Adapt to new cell types and events |
| MODIFY | `cli/src/tui/mod.rs` | Register new modules |
| DELETE | `cli/src/tui/chat_panel.rs` | Superseded by chat_widget.rs |
| MODIFY | `cli/src/tui/event.rs` | Add SlashCommand / Paste events |

## Phase 2: Popup/Overlay System

### Architecture
- `Overlay` trait with render + input handling
- `ViewStack` in BottomPane for push/pop overlay management
- Three overlay types:
  - `ApprovalOverlay` - tool call approval with approve/deny/edit options
  - `SelectionPopup` - generic list selection (model, theme, etc.)
  - `UserInputPopup` - multi-question form collection

### ApprovalOverlay
Replaces the current inline approval popup in `app.rs:546-641`:
- Full-width overlay with tool name, arguments, and action buttons
- Keyboard-driven: `a/y` approve, `d/n` deny, `e` edit
- Session-level "remember" toggle

## Phase 3: Status Indicator

### StatusIndicator Widget
- Animated spinner (shimmer/rotation effect using timer tick)
- Status text (current operation name)
- Elapsed time display
- Token usage progress bar: `[████░░░░] 82%`
- Rate limit warning at 75%/90%/95% thresholds

## Phase 4: Diff + Command Display

### Diff Rendering
- Syntactic diff display in AgentCell for patch content
- Green/red line coloring for additions/removals

### Command Execution Display
- CommandCell with pending → running → completed/failed states
- Animated state transitions
- Expandable output area

## Phase 5: Internal Event Bus

### AppEvent
```rust
pub enum AppEvent {
    StatusChanged(String),
    TokenUsage { used: u64, max: u64 },
    ModeChanged(Mode),
    ConfigChanged(String),
}
```
- `mpsc::unbounded_channel` for component-to-component communication
- Reduces direct state passing in App

## Self-Review
- [x] No placeholders or TODOs
- [x] Architecture matches existing patterns (ratatui + crossterm)
- [x] Scope focused on TUI only, no core library changes
- [x] Each phase independently deployable
- [x] Phase order respects dependency chain
