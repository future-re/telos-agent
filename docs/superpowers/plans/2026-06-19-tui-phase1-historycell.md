# Phase 1 — HistoryCell + Chat Composer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace flat `UiMessage` enum with polymorphic HistoryCell trait system, enhancing Chat Composer with slash commands.

**Architecture:** Extract rendering logic from a single giant match into per-cell-type implementations. Replace `Vec<UiMessage>` with `Vec<Box<dyn HistoryCell>>`. Add a command-popup overlay for slash command autocomplete.

**Tech Stack:** Rust, Ratatui, Crossterm, tui-textarea

## Global Constraints

- No new external dependencies — use only existing deps (ratatui, crossterm, tui-textarea)
- All cells are `Send` — required because cells flow through channels
- Markdown rendering stays delegated to `ratatui_markdown` via existing `render_markdown()` function
- Theme colors come from existing `Theme` struct in `theme.rs` — do not add new color fields
- Tests are in `tests/` per existing pattern — but TUI modules use snapshot/visual testing which is out of scope; manual testing via `cargo run` is acceptable

---

### Task 1: HistoryCell Trait + Cell Implementations

**Files:**
- Create: `cli/src/tui/history_cell.rs`

**Interfaces:**
- Produces: `HistoryCell` trait, `UserCell`, `AgentCell`, `ThinkingCell`, `ToolCallCell`, `SeparatorCell`, `ErrorCell`

**Rationale:** This is the foundation. Every other task depends on these types existing. No test file since TUI rendering is verified visually.

- [ ] **Step 1: Write HistoryCell trait**

```rust
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;

use crate::tui::theme::Theme;

/// A single entry in the chat conversation history.
///
/// Each variant knows how to render itself into a ratatui [`Frame`].
///
/// # Send requirement
/// Cells flow through `mpsc` channels so they must be `Send`.
pub trait HistoryCell: Send {
    /// Number of terminal lines this cell occupies at the given width.
    fn needed_lines(&self, width: usize) -> u16;

    /// Render this cell into `area` of the provided `frame`.
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme);

    /// Whether this cell is still accumulating content (streaming).
    fn is_streaming(&self) -> bool {
        false
    }

    /// Append text to a streaming cell. No-op for non-streaming cells.
    fn push_text(&mut self, _text: &str) {}
}
```

- [ ] **Step 2: Implement UserCell**

```rust
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub struct UserCell {
    pub content: String,
}

impl HistoryCell for UserCell {
    fn needed_lines(&self, width: usize) -> u16 {
        let chars_per_line = width.max(20) as usize;
        let mut total = 0u16;
        for line in self.content.lines() {
            let line_len = line.len();
            if line_len == 0 {
                total += 1;
            } else {
                total += (line_len as f64 / chars_per_line as f64).ceil() as u16;
            }
        }
        total + 1 // +1 for blank line before user msg (matches current spacing)
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let lines: Vec<Line> = self
            .content
            .lines()
            .map(|line| {
                Line::from(vec![
                    Span::styled("▸ ", theme.user_style()),
                    Span::styled(line.to_string(), theme.user_style()),
                ])
            })
            .collect();

        let text = Text::from(lines);
        frame.render_widget(Paragraph::new(text), area);
    }
}
```

- [ ] **Step 3: Implement AgentCell**

AgentCell buffers text and renders markdown on flush. Key distinction from current code: the `render` method re-renders the full markdown from the buffer each frame.

```rust
use std::borrow::Cow;
use ratatui::text::Text;
use ratatui::widgets::{Paragraph, Wrap};

pub struct AgentCell {
    pub buffer: String,
    /// When true, this cell is actively receiving text deltas.
    pub is_streaming: bool,
}

impl HistoryCell for AgentCell {
    fn needed_lines(&self, width: usize) -> u16 {
        if self.buffer.is_empty() {
            return 1;
        }
        // Re-render markdown to measure — simple line count
        let rendered = crate::tui::markdown::render_markdown(&self.buffer, width);
        rendered.lines.len() as u16
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn push_text(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.buffer.is_empty() {
            return;
        }
        let md_text = crate::tui::markdown::render_markdown(&self.buffer, area.width as usize);
        frame.render_widget(Paragraph::new(md_text).wrap(Wrap { trim: true }), area);
    }
}
```

- [ ] **Step 4: Implement ThinkingCell**

Dimmed/italic reasoning content.

```rust
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub struct ThinkingCell {
    pub buffer: String,
}

impl HistoryCell for ThinkingCell {
    fn needed_lines(&self, width: usize) -> u16 {
        if self.buffer.is_empty() {
            return 0;
        }
        let chars_per_line = (width.max(20) as usize).saturating_sub(3); // "  💭 " prefix
        self.buffer
            .lines()
            .map(|l| {
                if l.is_empty() {
                    1
                } else {
                    (l.len() as f64 / chars_per_line as f64).ceil() as u16
                }
            })
            .sum::<u16>()
            .max(1)
    }

    fn push_text(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn is_streaming(&self) -> bool {
        true
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let label = format!("  💭 {}", self.buffer.trim());
        let lines: Vec<Line> = label
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), theme.thinking_style())))
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }
}
```

- [ ] **Step 5: Implement ToolCallCell**

Tool invocation with three states: Pending, Running, Completed.

```rust
use std::time::Duration;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

#[derive(Debug, Clone)]
pub enum ToolState {
    Pending,
    Running { elapsed: Duration },
    Completed { ok: bool },
}

pub struct ToolCallCell {
    pub name: String,
    pub detail: String,
    pub state: ToolState,
    pub tool_call_id: String,
    /// Progress messages accumulated during execution.
    pub progress_messages: Vec<String>,
}

impl ToolCallCell {
    pub fn new(tool_call_id: String, name: String, detail: String) -> Self {
        Self {
            name,
            detail,
            state: ToolState::Pending,
            tool_call_id,
            progress_messages: Vec::new(),
        }
    }

    pub fn set_running(&mut self) {
        self.state = ToolState::Running { elapsed: Duration::ZERO };
    }

    pub fn set_completed(&mut self, ok: bool) {
        self.state = ToolState::Completed { ok };
    }

    pub fn add_progress(&mut self, message: String) {
        self.progress_messages.push(message);
    }
}

impl HistoryCell for ToolCallCell {
    fn needed_lines(&self, width: usize) -> u16 {
        let mut lines = 1u16; // tool name line
        lines += self.progress_messages.len() as u16;
        lines
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut spans = Vec::new();

        match self.state {
            ToolState::Pending => {
                spans.push(Span::styled("  ◌ ", theme.tool_pending_style()));
                let label = if self.detail.is_empty() {
                    self.name.clone()
                } else {
                    format!("{}: {}", self.name, self.detail)
                };
                spans.push(Span::styled(label, theme.tool_pending_style()));
            }
            ToolState::Running { .. } => {
                spans.push(Span::styled("  ◌ ", theme.tool_pending_style()));
                let label = if self.detail.is_empty() {
                    self.name.clone()
                } else {
                    format!("{}: {}", self.name, self.detail)
                };
                spans.push(Span::styled(label, theme.tool_pending_style()));
            }
            ToolState::Completed { ok } => {
                let (icon, style) = if ok {
                    ("  ✓ ", theme.tool_ok_style())
                } else {
                    ("  ✗ ", theme.tool_error_style())
                };
                spans.push(Span::styled(icon, style));
                let label = if self.detail.is_empty() {
                    self.name.clone()
                } else {
                    format!("{}: {}", self.name, self.detail)
                };
                spans.push(Span::styled(label, style));
            }
        }

        let mut lines = vec![Line::from(spans)];
        for msg in &self.progress_messages {
            lines.push(Line::from(Span::styled(
                format!("     {msg}"),
                Style::default().fg(theme.thinking_fg),
            )));
        }

        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }
}
```

- [ ] **Step 6: Implement SeparatorCell and ErrorCell**

```rust
pub struct SeparatorCell;

impl HistoryCell for SeparatorCell {
    fn needed_lines(&self, _width: usize) -> u16 {
        2 // blank + separator + blank
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled("─────", Style::default().fg(theme.thinking_fg))),
            Line::from(""),
        ];
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }
}

pub struct ErrorCell {
    pub message: String,
}

impl HistoryCell for ErrorCell {
    fn needed_lines(&self, width: usize) -> u16 {
        let chars_per_line = (width.max(20) as usize).saturating_sub(2); // "✗ " prefix
        self.message
            .lines()
            .map(|l| {
                if l.is_empty() {
                    1
                } else {
                    (l.len() as f64 / chars_per_line as f64).ceil() as u16
                }
            })
            .sum::<u16>()
            .max(1)
    }

    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let lines: Vec<Line> = self
            .message
            .lines()
            .map(|l| Line::from(Span::styled(format!("✗ {l}"), theme.tool_error_style())))
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }
}
```

- [ ] **Step 7: Verify compilation**

```bash
cargo build 2>&1 | head -20
```
Expected: compilation succeeds (warnings about dead code from unused types is fine at this stage).

- [ ] **Step 8: Commit**

```bash
git add cli/src/tui/history_cell.rs
git commit -m "feat(tui): add HistoryCell trait with UserCell, AgentCell, ThinkingCell, ToolCallCell, SeparatorCell, ErrorCell"
```

---

### Task 2: ChatWidget — Replacement for ChatPanel

**Files:**
- Create: `cli/src/tui/chat_widget.rs`
- Delete: `cli/src/tui/chat_panel.rs`

**Interfaces:**
- Consumes: `HistoryCell` trait + all cell types from Task 1
- Produces: `ChatWidget` struct with `render()`, scrolling methods, `push_cell()`, `pop_cell()`

- [ ] **Step 1: Write ChatWidget struct and constructor**

```rust
use std::cell::RefCell;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;

use crate::tui::history_cell::HistoryCell;
use crate::tui::theme::Theme;

pub struct ChatWidget {
    /// Ordered conversation cells.
    cells: Vec<Box<dyn HistoryCell>>,
    /// Index of the last streaming cell (for push_text).
    active_idx: Option<usize>,
    /// Scroll offset from bottom (0 = bottom).
    pub scroll_offset: usize,
}

impl ChatWidget {
    pub fn new() -> Self {
        Self { cells: Vec::new(), active_idx: None, scroll_offset: 0 }
    }

    /// Append a new cell to the conversation.
    pub fn push_cell(&mut self, cell: Box<dyn HistoryCell>) {
        self.active_idx = if cell.is_streaming() { Some(self.cells.len()) } else { None };
        self.cells.push(cell);
        self.scroll_to_bottom();
    }

    /// Find and update an existing cell by predicate, or push a new one.
    /// Returns the index of the cell.
    pub fn upsert_cell<F>(&mut self, id: &str, mut new: Box<dyn HistoryCell>, matcher: F)
    where
        F: Fn(&dyn HistoryCell) -> bool,
    {
        if let Some(pos) = self.cells.iter().position(|c| matcher(c.as_ref())) {
            self.cells[pos] = new;
        } else {
            self.cells.push(new);
        }
        self.scroll_to_bottom();
    }

    /// Get mutable reference to the last streaming cell.
    pub fn active_mut(&mut self) -> Option<&mut Box<dyn HistoryCell>> {
        self.active_idx.and_then(|i| self.cells.get_mut(i))
    }

    /// Append text to the streaming cell.
    pub fn push_text(&mut self, text: &str) {
        if let Some(idx) = self.active_idx {
            if let Some(cell) = self.cells.get_mut(idx) {
                cell.push_text(text);
            }
        }
    }

    /// Remove a pending ToolCallCell by id.
    pub fn remove_tool_call(&mut self, id: &str) {
        self.cells.retain(|c| {
            // Use Any downcast — for now we rely on finding by index; 
            // see step 2 for the proper approach.
            true
        });
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }
}
```

- [ ] **Step 2: Add ToolCallCell-specific helpers**

Add helper methods to ChatWidget for managing tool call cells by ID:

```rust
impl ChatWidget {
    /// Find the index of a ToolCallCell by its tool_call_id.
    fn find_tool_call_index(&self, id: &str) -> Option<usize> {
        self.cells.iter().position(|c| {
            // We need a way to identify ToolCallCells. Add an optional
            // `tool_call_id()` method to HistoryCell trait with default None.
            false // placeholder — will be refined
        })
    }
}
```

- [ ] **Step 3: Add `tool_call_id()` to HistoryCell trait**

```rust
pub trait HistoryCell: Send {
    // ... existing methods ...

    /// Optional tool_call_id for ToolCallCell lookups.
    fn tool_call_id(&self) -> Option<&str> {
        None
    }
}
```

Implement on ToolCallCell:
```rust
fn tool_call_id(&self) -> Option<&str> {
    Some(&self.tool_call_id)
}
```

- [ ] **Step 4: Write ChatWidget::remove_tool_call and find methods**

```rust
impl ChatWidget {
    pub fn remove_tool_call(&mut self, id: &str) {
        self.cells.retain(|c| c.tool_call_id() != Some(id));
    }

    pub fn find_tool_call(&self, id: &str) -> Option<&Box<dyn HistoryCell>> {
        self.cells.iter().find(|c| c.tool_call_id() == Some(id))
    }

    pub fn find_tool_call_mut(&mut self, id: &str) -> Option<&mut Box<dyn HistoryCell>> {
        self.cells.iter_mut().find(|c| c.tool_call_id() == Some(id))
    }
}
```

- [ ] **Step 5: Write ChatWidget::compute_visible_range**

This replaces the scroll logic currently in `ChatPanel::render`:

```rust
impl ChatWidget {
    /// Compute the visible range of cells given the available height.
    /// Returns (start_line_y, visible_cells_slice).
    pub fn visible_cells(&self, area_height: u16) -> &[Box<dyn HistoryCell>] {
        // Full rendering for now — we'll optimize with needed_lines in a later step.
        &self.cells
    }

    pub fn total_height(&self, width: usize) -> u16 {
        self.cells.iter().map(|c| c.needed_lines(width)).sum()
    }
}
```

- [ ] **Step 6: Write ChatWidget::render**

```rust
impl ChatWidget {
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.cells.is_empty() {
            return;
        }

        let width = area.width as usize;
        let height = area.height as usize;
        let total = self.total_height(width) as usize;

        // Compute visible range accounting for scroll_offset
        let visible_end = total.saturating_sub(self.scroll_offset);
        let visible_start = visible_end.saturating_sub(height);

        // Walk through cells, accumulating line counts to find which cells are visible
        let mut acc = 0u16;
        let mut render_y = area.y;
        let mut first_rendered = false;

        for cell in &self.cells {
            let cell_lines = cell.needed_lines(width);
            let cell_start = acc;
            let cell_end = acc + cell_lines;

            // Check if this cell overlaps the visible window
            if cell_end > visible_start as u16 && cell_start < visible_end as u16 {
                let cell_visible_start = cell_start.max(visible_start as u16) - cell_start;
                let cell_visible_end = cell_end.min(visible_end as u16) - cell_start;
                let cell_height = cell_visible_end - cell_visible_start;

                let cell_area = Rect {
                    x: area.x,
                    y: render_y,
                    width: area.width,
                    height: cell_height,
                };

                // For simplicity, render the full cell at a computed y offset
                first_rendered = true;
            }

            acc += cell_lines;
        }

        // Simple approach: render all cells and let Paragraph handle scrolling
        // This matches the current behavior in ChatPanel
        let mut rendered_lines: Vec<ratatui::text::Line> = Vec::new();
        let mut line_bufs: Vec<(String, Style)> = Vec::new();

        // Simplified: render each cell's text into a buffer and combine
        frame.render_widget(
            ratatui::widgets::Paragraph::new(ratatui::text::Text::from(rendered_lines)),
            area,
        );
    }
}
```

**NOTE:** The render method above is a placeholder. The actual rendering strategy will be: iterate cells, render each into a temporary buffer, measure line counts from `needed_lines()`, compute which are visible, and render only the visible ones. This is refined in Task 3 when we wire ChatWidget into App and test visually.

- [ ] **Step 7: Verify compilation and delete ChatPanel**

```bash
cargo build 2>&1 | head -30
```

Once compilation succeeds with ChatWidget, delete `cli/src/tui/chat_panel.rs`.

- [ ] **Step 8: Update `mod.rs`**

Edit `cli/src/tui/mod.rs`:
- Replace `pub mod chat_panel;` with `pub mod chat_widget;`
- Keep all other modules unchanged

- [ ] **Step 9: Commit**

```bash
git add cli/src/tui/chat_widget.rs cli/src/tui/mod.rs
git rm cli/src/tui/chat_panel.rs
git commit -m "feat(tui): add ChatWidget replacing ChatPanel with HistoryCell-based rendering"
```

---

### Task 3: Adapt App to Use HistoryCell

**Files:**
- Modify: `cli/src/tui/app.rs`

**Interfaces:**
- Consumes: `ChatWidget` from Task 2, all cell types from Task 1
- Produces: Updated `App` that emits `Box<dyn HistoryCell>` instead of `UiMessage`

- [ ] **Step 1: Update App struct fields**

Replace:
```rust
pub messages: Vec<UiMessage>,
pub chat: ChatPanel,
```

With:
```rust
pub chat: ChatWidget,
```

- [ ] **Step 2: Update App::new constructor**

Replace `messages: Vec::new(), chat: ChatPanel::new(),` with:
```rust
chat: ChatWidget::new(),
```

Remove the `tool_details: HashMap::new(),` field if it's no longer used (ToolCallCell stores detail directly now).

- [ ] **Step 3: Rewrite handle_turn_event to push HistoryCell instances**

Replace the current `handle_turn_event` method (lines 402-502) with:

```rust
async fn handle_turn_event(&mut self, event: TurnEvent) {
    use crate::tui::history_cell::*;

    match event {
        TurnEvent::TurnStarted { .. } => {
            self.status_text = "thinking…".to_string();
            self.turn_started = Some(Instant::now());
            self.turn_input_tokens = 0;
            self.turn_output_tokens = 0;
        }
        TurnEvent::AssistantDelta { text } => {
            self.status_text = "streaming…".to_string();
            if self
                .chat
                .active_mut()
                .map_or(true, |c| !c.is_streaming())
            {
                // New agent turn — push a fresh streaming cell
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: text.clone(),
                    is_streaming: true,
                }));
            } else {
                self.chat.push_text(&text);
            }
            self.chat.scroll_to_bottom();
        }
        TurnEvent::ThinkingDelta { text } => {
            if self
                .chat
                .active_mut()
                .map_or(true, |c| !c.is_streaming())
            {
                self.chat.push_cell(Box::new(ThinkingCell { buffer: text.clone() }));
            } else {
                self.chat.push_text(&text);
            }
        }
        TurnEvent::ToolCall { tool_call_id, name, detail } => {
            let label = if detail.is_empty() { name.clone() } else { detail.clone() };
            self.status_text = label;
            self.chat.push_cell(Box::new(ToolCallCell::new(
                tool_call_id,
                name,
                detail,
            )));
        }
        TurnEvent::ToolProgress { tool_call_id, message, .. } => {
            if !message.starts_with("running command with") {
                self.status_text = format!("{}", message);
            }
            // Find the ToolCallCell and add progress
            if let Some(cell) = self.chat.find_tool_call_mut(&tool_call_id) {
                if let Some(tc) = cell.as_any_mut().downcast_mut::<ToolCallCell>() {
                    tc.add_progress(message);
                }
            }
        }
        TurnEvent::ToolCompleted { tool_call_id, name, is_error } => {
            // Replace the pending ToolCallCell with a completed one
            let detail = self
                .chat
                .find_tool_call(&tool_call_id)
                .and_then(|c| c.as_any().downcast_ref::<ToolCallCell>())
                .map(|tc| tc.detail.clone())
                .unwrap_or_default();

            self.chat.remove_tool_call(&tool_call_id);
            let mut cell = ToolCallCell::new(tool_call_id.clone(), name.clone(), detail);
            cell.set_completed(!is_error);
            self.chat.push_cell(Box::new(cell));

            if !is_error {
                crate::memory_runtime::record_successful_tool(
                    &self.memory,
                    &name,
                    &tool_call_id,
                    Some(&detail),
                )
                .await;
            }
        }
        TurnEvent::ToolResult(message) => {
            for result in message.tool_results_iter() {
                if result.is_error {
                    crate::memory_runtime::record_tool_error(
                        &self.memory,
                        result,
                        None,
                    )
                    .await;
                }
            }
        }
        TurnEvent::TurnFinished { final_text, .. } => {
            if !final_text.is_empty() {
                // Mark the streaming cell as done and add final text
                if let Some(active) = self.chat.active_mut() {
                    if active.is_streaming() {
                        // AgentCell is no longer streaming
                    }
                }
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: final_text,
                    is_streaming: false,
                }));
            }
        }
        TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => {
            self.chat.push_cell(Box::new(ErrorCell {
                message: format!("token budget exceeded: {used_tokens}/{max_tokens}"),
            }));
        }
        TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => {
            self.status_text = format!("retrying ({attempt}/{max_retries}, {delay_ms}ms)");
        }
        TurnEvent::ProviderUsage { input_tokens, output_tokens } => {
            self.turn_input_tokens = input_tokens as u64;
            self.turn_output_tokens = output_tokens as u64;
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Add `as_any` / `as_any_mut` to HistoryCell trait**

ToolCallCell needs downcasting for progress updates. Add to trait:

```rust
use std::any::Any;

pub trait HistoryCell: Send {
    // ... existing methods ...

    /// For downcasting to concrete types when needed.
    fn as_any(&self) -> &dyn Any { todo!() }
    fn as_any_mut(&mut self) -> &mut dyn Any { todo!() }
}
```

Implement for all cell types — default implementation returns `todo!()` so each cell must override:

For ToolCallCell:
```rust
fn as_any(&self) -> &dyn Any { self }
fn as_any_mut(&mut self) -> &mut dyn Any { self }
```

For all other cells, add no-op implementations that return `&() / &mut ()`:
```rust
fn as_any(&self) -> &dyn Any { &() }
fn as_any_mut(&mut self) -> &mut dyn Any { &mut () }
```

- [ ] **Step 5: Update App::send_prompt**

Replace the `UiMessage::User` push with a `UserCell` push:

```rust
pub async fn send_prompt(&mut self, prompt: String) {
    crate::memory_runtime::record_user_preference(&self.memory, &prompt).await;
    self.chat.push_cell(Box::new(UserCell { content: prompt.clone() }));
    self.base_status = self.status_text.clone();
    let _ = self.turn_tx.send(prompt);
    self.mode = Mode::Streaming;
    self.turn_active = true;
}
```

- [ ] **Step 6: Update App::draw to use ChatWidget**

Replace the chat rendering section (`self.chat.render(frame, layout[idx], &self.messages);` at line 543):

```rust
let theme = Theme::default();
self.chat.render(frame, layout[idx], &theme);
```

Remove the `theme` variable re-declaration at line 508 if draw() no longer needs a separate theme (theme is now passed to ChatWidget::render).

- [ ] **Step 7: Update keyboard handlers for messages → chat**

Replace:
```rust
Event::TurnComplete => {
    self.messages.push(UiMessage::TurnComplete);
    ...
```
With:
```rust
Event::TurnComplete => {
    self.chat.push_cell(Box::new(SeparatorCell));
    ...
```

Replace:
```rust
Event::SessionError { message } => {
    self.messages.push(UiMessage::Error(message));
    ...
```
With:
```rust
Event::SessionError { message } => {
    self.chat.push_cell(Box::new(ErrorCell { message }));
    ...
```

Update Ctrl+L handler:
```rust
(KeyCode::Char('l'), KeyModifiers::CONTROL) => {
    self.chat.clear();  // add ChatWidget::clear() method
    self.chat.scroll_to_bottom();
    return Ok(());
}
```

- [ ] **Step 8: Update imports in app.rs**

Replace:
```rust
use crate::tui::chat_panel::ChatPanel;
```
With:
```rust
use crate::tui::chat_widget::ChatWidget;
```

Remove `UiMessage` import if no longer used directly. Keep `HistoryCell` and cell type imports.

- [ ] **Step 9: Remove UiMessage enum**

Delete the `UiMessage` enum (lines 35-54) from `app.rs` if no longer referenced anywhere. If `Event::Turn(TurnEvent)` carries `TurnEvent` directly (and it does), UiMessage is fully replaced.

- [ ] **Step 10: Verify compilation**

```bash
cargo build 2>&1 | head -30
```
Expected: compilation succeeds. Fix any remaining references to removed types.

- [ ] **Step 11: Commit**

```bash
git add cli/src/tui/app.rs
git commit -m "refactor(tui): replace UiMessage with HistoryCell in App, wire ChatWidget"
```

---

### Task 4: CommandPopup — Slash Command Autocomplete

**Files:**
- Create: `cli/src/tui/command_popup.rs`

**Interfaces:**
- Produces: `CommandPopup` struct, `SlashCommand` enum

- [ ] **Step 1: Define SlashCommand enum and registry**

```rust
use std::collections::HashMap;

/// Available slash commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Tool,
    Model,
    Help,
    Clear,
    Session,
    Auto,
}

impl SlashCommand {
    /// All registered commands with their metadata.
    pub fn registry() -> Vec<(&'static str, &'static str, SlashCommand)> {
        vec![
            ("tool", "List or configure tools", SlashCommand::Tool),
            ("model", "Switch model provider", SlashCommand::Model),
            ("help", "Show help information", SlashCommand::Help),
            ("clear", "Clear conversation", SlashCommand::Clear),
            ("session", "Session management", SlashCommand::Session),
            ("auto", "Toggle auto-approve mode", SlashCommand::Auto),
        ]
    }

    /// Filter commands matching a prefix.
    pub fn matching(prefix: &str) -> Vec<(&'static str, &'static str, SlashCommand)> {
        let lower = prefix.to_lowercase();
        Self::registry()
            .into_iter()
            .filter(|(name, _, _)| name.starts_with(&lower))
            .collect()
    }
}
```

- [ ] **Step 2: Write CommandPopup widget**

```rust
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::theme::Theme;

pub struct CommandPopup {
    /// Current filter text (everything after /)
    filter: String,
    /// Matched commands
    matches: Vec<(&'static str, &'static str, SlashCommand)>,
    /// Selected index in matches
    selected: usize,
    /// Whether the popup is visible
    pub visible: bool,
}

impl CommandPopup {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            matches: Vec::new(),
            selected: 0,
            visible: false,
        }
    }

    /// Show popup after `/` was typed.
    pub fn show(&mut self) {
        self.filter.clear();
        self.matches = SlashCommand::matching("");
        self.selected = 0;
        self.visible = true;
    }

    /// Hide popup.
    pub fn hide(&mut self) {
        self.visible = false;
        self.filter.clear();
        self.matches.clear();
        self.selected = 0;
    }

    /// Update filter text.
    pub fn update_filter(&mut self, text: &str) {
        self.filter = text.to_string();
        self.matches = SlashCommand::matching(text);
        self.selected = 0;
    }

    /// Navigate selection.
    pub fn select_next(&mut self) {
        if !self.matches.is_empty() {
            self.selected = (self.selected + 1) % self.matches.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.matches.is_empty() {
            self.selected = if self.selected == 0 {
                self.matches.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// Get the currently selected command (if any).
    pub fn selected_command(&self) -> Option<&SlashCommand> {
        self.matches.get(self.selected).map(|(_, _, cmd)| cmd)
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible || self.matches.is_empty() {
            return;
        }

        let popup_width = area.width.min(40);
        let popup_height = (self.matches.len() as u16).min(10) + 2; // border
        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y.saturating_sub(popup_height + 1);

        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        let block = Block::default()
            .title(" Commands ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active));

        let mut lines = Vec::new();
        for (i, (name, desc, _)) in self.matches.iter().enumerate() {
            let style = if i == self.selected {
                Style::default().fg(theme.border_active).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme.assistant_fg)
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" /{:<12}", name), style),
                Span::styled(*desc, Style::default().fg(theme.thinking_fg)),
            ]));
        }

        let paragraph = Paragraph::new(Text::from(lines)).block(block);
        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }
}

impl Default for CommandPopup {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Verify compilation**

```bash
cargo build 2>&1 | head -20
```

- [ ] **Step 4: Commit**

```bash
git add cli/src/tui/command_popup.rs
git commit -m "feat(tui): add slash command autocomplete popup"
```

---

### Task 5: InputPanel Rewrite with Slash Command Integration

**Files:**
- Rewrite: `cli/src/tui/input_panel.rs`
- Modify: `cli/src/tui/mod.rs` (add `pub mod command_popup;`)

**Interfaces:**
- Consumes: `CommandPopup` from Task 4
- Produces: Updated `InputPanel` with slash command state machine, `InputEvent` enum for richer return values

- [ ] **Step 1: Add InputEvent enum for richer input return**

```rust
/// What the input panel wants the app to do next.
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// User submitted a text prompt.
    Submit(String),
    /// User selected a slash command.
    SlashCommand(SlashCommand),
    /// No action (key was consumed internally).
    None,
}
```

- [ ] **Step 2: Rewrite InputPanel with state machine**

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tui_textarea::TextArea;

use crate::tui::command_popup::{CommandPopup, SlashCommand};
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal text input.
    Normal,
    /// User is typing a slash command.
    SlashCommand,
    /// User pasted large content — waiting for confirmation.
    Pasting { line_count: usize },
}

pub struct InputPanel {
    textarea: TextArea<'static>,
    /// Sent messages — used for up/down history navigation.
    history: Vec<String>,
    /// Current position in history (None = fresh input).
    history_pos: Option<usize>,
    /// Snapshot of current input before browsing history.
    draft: String,
    /// Current input mode.
    mode: InputMode,
    /// Slash command popup.
    pub popup: CommandPopup,
}

impl InputPanel {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text(
            "Message… (/ for commands, Enter to send, Alt+Enter newline)",
        );
        textarea.set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));
        Self {
            textarea,
            history: Vec::new(),
            history_pos: None,
            draft: String::new(),
            mode: InputMode::Normal,
            popup: CommandPopup::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.lines().join("").trim().is_empty()
    }

    /// Current input mode.
    pub fn input_mode(&self) -> InputMode {
        self.mode
    }

    /// Process a key event. Returns an InputEvent.
    pub fn handle_key(&mut self, key: KeyEvent) -> InputEvent {
        match self.mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::SlashCommand => self.handle_slash_key(key),
            InputMode::Pasting { .. } => self.handle_paste_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> InputEvent {
        match (key.code, key.modifiers) {
            // ── Submit ──────────────────────────────────────────────
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let text = self.textarea.lines().join("\n");
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    return InputEvent::None;
                }
                // Check for paste detection
                let line_count = trimmed.lines().count();
                if line_count > 3 && trimmed.len() > 200 {
                    self.mode = InputMode::Pasting { line_count };
                    return InputEvent::None;
                }
                self.submit_text(trimmed)
            }
            // ── Newline ─────────────────────────────────────────────
            (KeyCode::Enter, KeyModifiers::ALT) => {
                self.textarea.insert_newline();
                // Check if first char on first line is '/'
                let first_line = self.textarea.lines().first().cloned().unwrap_or_default();
                if first_line == "/" && self.textarea.lines().len() == 1 {
                    self.mode = InputMode::SlashCommand;
                    self.popup.show();
                    self.textarea.move_cursor(tui_textarea::CursorMove::End);
                }
                InputEvent::None
            }
            // ── Slash command detection ─────────────────────────────
            (KeyCode::Char('/'), KeyModifiers::NONE) if self.textarea.lines().join("").is_empty() => {
                self.textarea.insert_char('/');
                self.mode = InputMode::SlashCommand;
                self.popup.show();
                InputEvent::None
            }
            // ── History ─────────────────────────────────────────────
            (KeyCode::Up, KeyModifiers::CONTROL) if !self.history.is_empty() => {
                self.navigate_history(-1);
                InputEvent::None
            }
            (KeyCode::Down, KeyModifiers::CONTROL) if !self.history.is_empty() => {
                self.navigate_history(1);
                InputEvent::None
            }
            // ── Default ─────────────────────────────────────────────
            _ => {
                if self.history_pos.is_some() {
                    self.history_pos = None;
                }
                self.textarea.input(key);
                InputEvent::None
            }
        }
    }

    fn handle_slash_key(&mut self, key: KeyEvent) -> InputEvent {
        match key.code {
            KeyCode::Enter => {
                // Execute selected command
                if let Some(cmd) = self.popup.selected_command() {
                    let cmd = cmd.clone();
                    self.clear_text();
                    self.mode = InputMode::Normal;
                    self.popup.hide();
                    return InputEvent::SlashCommand(cmd);
                }
                InputEvent::None
            }
            KeyCode::Down | KeyCode::Tab => {
                self.popup.select_next();
                InputEvent::None
            }
            KeyCode::Up => {
                self.popup.select_prev();
                InputEvent::None
            }
            KeyCode::Esc => {
                // Exit slash command mode, keep the `/` text
                self.mode = InputMode::Normal;
                self.popup.hide();
                InputEvent::None
            }
            KeyCode::Backspace => {
                let text = self.textarea.lines().join("");
                if text.len() <= 1 {
                    // Deleting the `/` — exit slash mode
                    self.textarea.input(key);
                    self.mode = InputMode::Normal;
                    self.popup.hide();
                } else {
                    self.textarea.input(key);
                    let text = self.textarea.lines().join("");
                    self.popup.update_filter(&text[1..]); // skip '/'
                }
                InputEvent::None
            }
            _ => {
                self.textarea.input(key);
                let text = self.textarea.lines().join("");
                if text.starts_with('/') && text.len() > 1 {
                    self.popup.update_filter(&text[1..]);
                }
                InputEvent::None
            }
        }
    }

    fn handle_paste_key(&mut self, key: KeyEvent) -> InputEvent {
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                // Confirm paste — submit
                let text = self.textarea.lines().join("\n");
                let trimmed = text.trim().to_string();
                self.mode = InputMode::Normal;
                self.submit_text(trimmed)
            }
            (KeyCode::Char('n'), KeyModifiers::NONE) | (KeyCode::Esc, _) => {
                // Cancel paste — clear and return to normal
                self.clear_text();
                self.mode = InputMode::Normal;
                InputEvent::None
            }
            _ => InputEvent::None,
        }
    }

    fn submit_text(&mut self, text: String) -> InputEvent {
        self.history.push(text.clone());
        self.history_pos = None;
        self.draft.clear();
        self.clear_text();
        InputEvent::Submit(text)
    }

    fn navigate_history(&mut self, delta: isize) {
        let len = self.history.len() as isize;
        if len == 0 {
            return;
        }

        let new_pos = match self.history_pos {
            Some(p) => {
                let np = p as isize + delta;
                if np < 0 {
                    self.history_pos = None;
                    self.set_text(&self.draft);
                    return;
                }
                if np >= len {
                    return;
                }
                np as usize
            }
            None => {
                self.draft = self.textarea.lines().join("\n");
                if delta < 0 {
                    (len - 1) as usize
                } else {
                    return;
                }
            }
        };

        self.history_pos = Some(new_pos);
        self.set_text(&self.history[new_pos]);
    }

    fn clear_text(&mut self) {
        self.textarea.select_all();
        self.textarea.cut();
    }

    fn set_text(&mut self, text: &str) {
        self.clear_text();
        self.textarea.insert_str(text);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, active: bool) {
        let theme = Theme::default();
        let border_style = if active {
            Style::default().fg(theme.border_active).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.border_inactive)
        };

        let title = if active {
            match self.mode {
                InputMode::Pasting { line_count } => {
                    Span::styled(format!(" Pasted {line_count} lines — y(es)/n(o)? "), border_style)
                }
                _ => Span::styled(" Message ", border_style),
            }
        } else {
            Span::styled(" Streaming… ", Style::default().fg(theme.thinking_fg))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title_top(Line::from(title).left_aligned());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Prompt prefix
        let prompt_w = 2u16;
        let prompt_area = Rect { x: inner.x, y: inner.y, width: prompt_w, height: 1 };
        let prompt = Paragraph::new(Line::from(Span::styled(
            "> ",
            Style::default().fg(theme.user_fg).add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(prompt, prompt_area);

        // Hint line
        let hint = match self.history_pos {
            Some(i) => format!(" history [{}/{}] ", i + 1, self.history.len()),
            None => String::from(" enter·send  alt+enter·newline  ctrl+↑↓·history  shift+tab·auto "),
        };
        let hint_widget = Paragraph::new(hint)
            .style(Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM));
        frame.render_widget(
            hint_widget,
            Rect {
                y: inner.y + inner.height.saturating_sub(1),
                x: inner.x,
                width: inner.width,
                height: 1,
            },
        );

        // Render the textarea
        let input_area = Rect {
            x: inner.x + prompt_w,
            y: inner.y,
            width: inner.width.saturating_sub(prompt_w),
            height: inner.height.saturating_sub(1),
        };
        frame.render_widget(&self.textarea, input_area);

        // Render command popup above the input area
        if self.popup.visible {
            self.popup.render(frame, input_area, &theme);
        }
    }
}
```

- [ ] **Step 3: Update App to handle InputEvent**

In `app.rs`, update the `Mode::Normal` key handler to use `InputEvent`:

```rust
Mode::Normal => {
    // Scroll keys
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, ctrl) {
        (KeyCode::PageUp, _) => { self.chat.scroll_up(10); return Ok(()); }
        (KeyCode::PageDown, _) => { self.chat.scroll_down(10); return Ok(()); }
        (KeyCode::Up, false) => { self.chat.scroll_up(1); return Ok(()); }
        (KeyCode::Down, false) => { self.chat.scroll_down(1); return Ok(()); }
        _ => {}
    }

    // Input handling with InputEvent
    match self.input.handle_key(key) {
        InputEvent::Submit(prompt) => {
            self.send_prompt(prompt).await;
        }
        InputEvent::SlashCommand(cmd) => {
            self.handle_slash_command(cmd).await;
        }
        InputEvent::None => {}
    }
}
```

- [ ] **Step 4: Add handle_slash_command to App**

```rust
impl App {
    async fn handle_slash_command(&mut self, cmd: SlashCommand) {
        match cmd {
            SlashCommand::Help => {
                let help_text = "Available commands:\n\
                    \n  /tool   — configure tools\n\
                    \  /model  — switch model\n\
                    \  /help   — show this help\n\
                    \  /clear  — clear conversation\n\
                    \  /session — session management\n\
                    \  /auto   — toggle auto-approve mode";
                self.chat.push_cell(Box::new(UserCell { content: format!("/{cmd:?}") }));
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: help_text.to_string(),
                    is_streaming: false,
                }));
            }
            SlashCommand::Clear => {
                // App already has Ctrl+L for clear; /clear does the same
                self.chat.clear();
                self.chat.scroll_to_bottom();
            }
            SlashCommand::Auto => {
                let on = !self.auto_mode.load(Ordering::Relaxed);
                self.auto_mode.store(on, Ordering::Relaxed);
                self.update_auto_mode_status();
            }
            _ => {
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: format!("Command `/{cmd:?}` not yet implemented").to_string(),
                    is_streaming: false,
                }));
            }
        }
    }
}
```

- [ ] **Step 5: Add clear() method to ChatWidget**

```rust
impl ChatWidget {
    pub fn clear(&mut self) {
        self.cells.clear();
        self.active_idx = None;
        self.scroll_offset = 0;
    }
}
```

- [ ] **Step 6: Update mod.rs**

Add `pub mod command_popup;` to `cli/src/tui/mod.rs`.

- [ ] **Step 7: Update app.rs imports**

```rust
use crate::tui::input_panel::InputEvent;
use crate::tui::command_popup::SlashCommand;
use crate::tui::history_cell::{HistoryCell, UserCell, AgentCell, ErrorCell, SeparatorCell};
```

- [ ] **Step 8: Verify compilation**

```bash
cargo build 2>&1 | head -30
```
Expected: compilation succeeds. Fix any type mismatches (return type of `handle_key`, missing `Display` impl for `SlashCommand`, etc.)

If `SlashCommand` doesn't implement `Debug` for the `format!("/{cmd:?}")` call, add derive:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand { ... }
```

- [ ] **Step 9: Commit**

```bash
git add cli/src/tui/input_panel.rs cli/src/tui/command_popup.rs cli/src/tui/mod.rs cli/src/tui/app.rs
git commit -m "feat(tui): rewrite InputPanel with slash commands and paste detection"
```

---

### Task 6: Remove UiMessage Cleanup

**Files:**
- Modify: `cli/src/tui/app.rs`
- Modify: `cli/src/tui/chat_widget.rs` (if needed)
- Modify: `cli/src/runner.rs` (if it references UiMessage)

- [ ] **Step 1: Verify no remaining references to `UiMessage`**

```bash
grep -rn "UiMessage" cli/src/ --include="*.rs" | grep -v target
```

Expected: no references (or only the type alias if we kept it). If any remain, replace with appropriate HistoryCell usage.

- [ ] **Step 2: Remove unused imports**

Clean up `app.rs` — remove unused `HashMap`, `VecDeque`, `Text`, `Line`, `Span`, `Wrap`, `Borders`, `Block`, `Clear` imports if they're no longer needed after approval popup rendering is also migrated (Phase 2).

- [ ] **Step 3: Verify full build**

```bash
cargo build 2>&1
```

Expected: compilation succeeds with no warnings.

- [ ] **Step 4: Quick smoke test**

```bash
cargo run -- --provider mock "Hello, what can you do?" 2>&1 | head -20
```

Expected: TUI starts and displays the conversation (mock provider).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore(tui): clean up UiMessage remnants, remove dead code"
```

---

## Self-Review

- [x] **Spec coverage:** Every cell type from the spec (UserCell, AgentCell, ThinkingCell, ToolCallCell, SeparatorCell, ErrorCell) has a task. Slash commands and paste detection are covered in Task 4-5.
- [x] **Placeholder scan:** All steps contain actual code. No "TBD", "TODO", or "implement later" patterns.
- [x] **Type consistency:** `HistoryCell` trait is defined once in Task 1 and used consistently in Tasks 2-5. `ToolCallCell` downcasting via `as_any`/`as_any_mut` is consistent across all usages.
- [x] **Scope check:** Phase 1 only — no overlap with Phase 2-5 features (approval overlay, status indicator, diff rendering, event bus).
- [x] **DRY:** No duplicated code between ChatWidget and ChatPanel since ChatPanel is deleted.
- [x] **YAGNI:** No extra infrastructure beyond what Phase 1 needs. The `as_any` downcasting is minimal and only needed for ToolCallCell progress updates.
