# Inline Approval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the centered TUI approval popup with a compact inline approval card above the composer while allowing users to draft input during agent execution.

**Architecture:** Keep the core approval API and `TuiApprovalHandler` unchanged. Move approval ownership into `App` as an active `PendingApproval` plus FIFO queue, render it with a focused inline widget, and route approval shortcuts before normal streaming input handling. Keep edit approval arguments on the existing `UserInputPopup` overlay for this iteration.

**Tech Stack:** Rust 2024, Tokio channels, Ratatui widgets, crossterm key events, existing `telos_agent` approval types.

---

## File Structure

- Create `cli/src/tui/widgets/approval_inline.rs`: pure rendering helpers for inline approval text and the Ratatui panel.
- Modify `cli/src/tui/mod.rs`: expose the new approval inline widget module.
- Modify `cli/src/tui/app/mod.rs`: add approval state, queue, layout row, and app-level tests.
- Modify `cli/src/tui/app/events.rs`: enqueue approval requests, route inline approval shortcuts, and allow streaming composer edits.
- Modify `cli/src/tui/app/commands.rs`: prevent concurrent prompt dispatch during streaming and add inline approval resolution helpers.
- Modify `cli/src/tui/widgets/input_panel.rs`: expose a read-only `text()` helper for app tests and streaming submit behavior.

## Task 1: Input Panel Text Access

**Files:**
- Modify: `cli/src/tui/widgets/input_panel.rs`

- [ ] **Step 1: Write the failing test**

Add this test inside `cli/src/tui/widgets/input_panel.rs` existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn text_returns_multiline_composer_contents() {
    let mut panel = InputPanel::new();
    set_text(&mut panel, "line one\nline two");

    assert_eq!(panel.text(), "line one\nline two");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p telos-cli tui::input_panel::tests::text_returns_multiline_composer_contents
```

Expected: FAIL with a compiler error that `InputPanel` has no method named `text`.

- [ ] **Step 3: Write minimal implementation**

Add this method to `impl InputPanel` near `is_empty`:

```rust
pub fn text(&self) -> String {
    self.textarea.lines().join("\n")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p telos-cli tui::input_panel::tests::text_returns_multiline_composer_contents
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add cli/src/tui/widgets/input_panel.rs
git commit -m "test: expose composer text for tui assertions"
```

## Task 2: Inline Approval State And Resolution

**Files:**
- Modify: `cli/src/tui/app/mod.rs`
- Modify: `cli/src/tui/app/commands.rs`

- [ ] **Step 1: Write the failing tests**

Add these helpers and tests inside `cli/src/tui/app/mod.rs` existing `#[cfg(test)] mod tests`:

```rust
use serde_json::json;
use std::collections::VecDeque;
use std::path::PathBuf;
use telos_agent::{ApprovalDecision, ApprovalRequest, Message};
use tokio::sync::oneshot;

fn approval_request(command: &str) -> ApprovalRequest {
    ApprovalRequest {
        tool_name: "Bash".into(),
        invocation_names: vec!["Bash".into(), "shell".into()],
        arguments: json!({ "command": command }),
        cwd: PathBuf::from("."),
        messages: Arc::new(vec![Message::user("run a command")]),
        reason: "command requires approval".into(),
    }
}

#[test]
fn enqueue_pending_approval_sets_active_without_approving_mode() {
    let config = telos_agent::AgentConfig::default();
    let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
    let tools = telos_agent::ToolRegistry::new();
    let temp = tempfile::tempdir().unwrap();
    let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));
    let mut app = App::new(
        config,
        provider,
        tools,
        "telos".into(),
        Some(temp.path()),
        temp.path(),
        false,
        memory,
        ModelSwitchConfig::default(),
    )
    .unwrap();
    let (tx, _rx) = oneshot::channel();

    app.enqueue_inline_approval(PendingApproval {
        request: approval_request("rm target"),
        respond: Some(tx),
    });

    assert!(app.inline_approval.is_some());
    assert_eq!(app.inline_approval_queue.len(), 0);
    assert_ne!(app.mode, Mode::Approving);
}

#[tokio::test]
async fn inline_approval_shortcuts_resolve_allow_and_deny() {
    let config = telos_agent::AgentConfig::default();
    let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
    let tools = telos_agent::ToolRegistry::new();
    let temp = tempfile::tempdir().unwrap();
    let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));
    let mut app = App::new(
        config,
        provider,
        tools,
        "telos".into(),
        Some(temp.path()),
        temp.path(),
        false,
        memory,
        ModelSwitchConfig::default(),
    )
    .unwrap();
    let (allow_tx, allow_rx) = oneshot::channel();
    app.enqueue_inline_approval(PendingApproval {
        request: approval_request("echo allow"),
        respond: Some(allow_tx),
    });

    app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    )))
    .await
    .unwrap();

    assert_eq!(allow_rx.await.unwrap(), ApprovalDecision::Allow);
    assert!(app.inline_approval.is_none());

    let (deny_tx, deny_rx) = oneshot::channel();
    app.enqueue_inline_approval(PendingApproval {
        request: approval_request("echo deny"),
        respond: Some(deny_tx),
    });

    app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('n'),
        crossterm::event::KeyModifiers::NONE,
    )))
    .await
    .unwrap();

    assert!(matches!(
        deny_rx.await.unwrap(),
        ApprovalDecision::Deny { reason } if reason == "denied by user"
    ));
    assert!(app.inline_approval.is_none());
}

#[test]
fn enqueue_pending_approvals_keeps_fifo_order() {
    let config = telos_agent::AgentConfig::default();
    let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
    let tools = telos_agent::ToolRegistry::new();
    let temp = tempfile::tempdir().unwrap();
    let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));
    let mut app = App::new(
        config,
        provider,
        tools,
        "telos".into(),
        Some(temp.path()),
        temp.path(),
        false,
        memory,
        ModelSwitchConfig::default(),
    )
    .unwrap();
    let (first_tx, _first_rx) = oneshot::channel();
    let (second_tx, _second_rx) = oneshot::channel();

    app.enqueue_inline_approval(PendingApproval {
        request: approval_request("first"),
        respond: Some(first_tx),
    });
    app.enqueue_inline_approval(PendingApproval {
        request: approval_request("second"),
        respond: Some(second_tx),
    });

    assert_eq!(
        app.inline_approval.as_ref().unwrap().request.arguments["command"],
        "first"
    );
    assert_eq!(app.inline_approval_queue.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p telos-cli tui::app::tests::enqueue_pending_approval_sets_active_without_approving_mode tui::app::tests::inline_approval_shortcuts_resolve_allow_and_deny tui::app::tests::enqueue_pending_approvals_keeps_fifo_order
```

Expected: FAIL with compiler errors for missing `inline_approval`, `inline_approval_queue`, and `enqueue_inline_approval`.

- [ ] **Step 3: Add app state**

In `cli/src/tui/app/mod.rs`, add:

```rust
use std::collections::VecDeque;
```

Add fields to `App` near `editing_approval`:

```rust
/// Approval request currently shown in the inline approval panel.
inline_approval: Option<PendingApproval>,
/// Pending approval requests waiting for the inline panel.
inline_approval_queue: VecDeque<PendingApproval>,
```

Initialize them in `App::new_with_layout_settings`:

```rust
inline_approval: None,
inline_approval_queue: VecDeque::new(),
```

- [ ] **Step 4: Add resolution helpers**

In `cli/src/tui/app/commands.rs`, add `ApprovalDecision` to the imports:

```rust
use telos_agent::ApprovalDecision;
```

Add these methods to `impl App`:

```rust
pub(super) fn enqueue_inline_approval(&mut self, pending: PendingApproval) {
    if self.inline_approval.is_none() {
        self.inline_approval = Some(pending);
    } else {
        self.inline_approval_queue.push_back(pending);
    }
}

pub(super) fn resolve_inline_approval(&mut self, decision: ApprovalDecision) {
    if let Some(mut pending) = self.inline_approval.take()
        && let Some(tx) = pending.respond.take()
    {
        if tx.send(decision).is_err() {
            self.status_text = "approval response channel closed".to_string();
        }
    }
    self.inline_approval = self.inline_approval_queue.pop_front();
}

pub(super) fn open_inline_approval_edit_popup(&mut self) {
    if let Some(mut pending) = self.inline_approval.take() {
        let next = self.inline_approval_queue.pop_front();
        self.open_approval_edit_popup(PendingApproval {
            request: pending.request.clone(),
            respond: pending.respond.take(),
        });
        self.inline_approval = next;
    }
}
```

- [ ] **Step 5: Route inline approval shortcuts**

In `cli/src/tui/app/events.rs`, before the `match self.mode` block inside `Event::Key`, add:

```rust
if self.inline_approval.is_some() {
    match key.code {
        KeyCode::Char('a') | KeyCode::Char('y') => {
            self.resolve_inline_approval(telos_agent::ApprovalDecision::Allow);
            return Ok(());
        }
        KeyCode::Char('d') | KeyCode::Char('n') => {
            self.resolve_inline_approval(telos_agent::ApprovalDecision::Deny {
                reason: "denied by user".into(),
            });
            return Ok(());
        }
        KeyCode::Char('e') => {
            self.open_inline_approval_edit_popup();
            return Ok(());
        }
        _ => {}
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run:

```bash
cargo test -p telos-cli tui::app::tests::enqueue_pending_approval_sets_active_without_approving_mode tui::app::tests::inline_approval_shortcuts_resolve_allow_and_deny tui::app::tests::enqueue_pending_approvals_keeps_fifo_order
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add cli/src/tui/app/mod.rs cli/src/tui/app/commands.rs cli/src/tui/app/events.rs
git commit -m "feat: track inline approval state"
```

## Task 3: Approval Channel Uses Inline State

**Files:**
- Modify: `cli/src/tui/app/events.rs`
- Modify: `cli/src/tui/app/mod.rs`

- [ ] **Step 1: Write the failing test**

Add this test inside `cli/src/tui/app/mod.rs` existing tests:

```rust
#[tokio::test]
async fn approval_channel_tick_uses_inline_state_instead_of_overlay() {
    let config = telos_agent::AgentConfig::default();
    let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
    let tools = telos_agent::ToolRegistry::new();
    let temp = tempfile::tempdir().unwrap();
    let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));
    let mut app = App::new(
        config,
        provider,
        tools,
        "telos".into(),
        Some(temp.path()),
        temp.path(),
        false,
        memory,
        ModelSwitchConfig::default(),
    )
    .unwrap();
    let (tx, _rx) = oneshot::channel();

    app.approval_rx.close();
    app.approval_rx = {
        let (approval_tx, approval_rx) = tokio::sync::mpsc::unbounded_channel();
        approval_tx
            .send(PendingApproval {
                request: approval_request("echo inline"),
                respond: Some(tx),
            })
            .unwrap();
        approval_rx
    };

    app.handle_event(Event::Tick).await.unwrap();

    assert!(app.inline_approval.is_some());
    assert!(app.overlays.is_empty());
    assert_ne!(app.mode, Mode::Approving);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p telos-cli tui::app::tests::approval_channel_tick_uses_inline_state_instead_of_overlay
```

Expected: FAIL because the existing tick handler pushes `ApprovalOverlay` and sets `Mode::Approving`.

- [ ] **Step 3: Update approval channel handling**

In `cli/src/tui/app/events.rs`, replace:

```rust
while let Ok(pending) = self.approval_rx.try_recv() {
    self.overlays.push(Box::new(ApprovalOverlay::new(pending)));
    self.mode = Mode::Approving;
}
```

with:

```rust
while let Ok(pending) = self.approval_rx.try_recv() {
    self.enqueue_inline_approval(pending);
}
```

Remove the unused `ApprovalOverlay` import from `cli/src/tui/app/events.rs`:

```rust
use crate::tui::overlay::OverlayAction;
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p telos-cli tui::app::tests::approval_channel_tick_uses_inline_state_instead_of_overlay
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add cli/src/tui/app/events.rs cli/src/tui/app/mod.rs
git commit -m "feat: route tui approvals inline"
```

## Task 4: Streaming Composer Editing

**Files:**
- Modify: `cli/src/tui/app/events.rs`
- Modify: `cli/src/tui/app/commands.rs`
- Modify: `cli/src/tui/app/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests inside `cli/src/tui/app/mod.rs` existing tests:

```rust
#[tokio::test]
async fn streaming_character_input_updates_composer() {
    let config = telos_agent::AgentConfig::default();
    let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
    let tools = telos_agent::ToolRegistry::new();
    let temp = tempfile::tempdir().unwrap();
    let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));
    let mut app = App::new(
        config,
        provider,
        tools,
        "telos".into(),
        Some(temp.path()),
        temp.path(),
        false,
        memory,
        ModelSwitchConfig::default(),
    )
    .unwrap();
    app.mode = Mode::Streaming;
    app.turn_active = true;

    app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('h'),
        crossterm::event::KeyModifiers::NONE,
    )))
    .await
    .unwrap();
    app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('i'),
        crossterm::event::KeyModifiers::NONE,
    )))
    .await
    .unwrap();

    assert_eq!(app.input.text(), "hi");
    assert_eq!(app.mode, Mode::Streaming);
    assert!(app.turn_active);
}

#[tokio::test]
async fn streaming_enter_keeps_draft_and_does_not_dispatch_prompt() {
    let config = telos_agent::AgentConfig::default();
    let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
    let tools = telos_agent::ToolRegistry::new();
    let temp = tempfile::tempdir().unwrap();
    let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));
    let mut app = App::new(
        config,
        provider,
        tools,
        "telos".into(),
        Some(temp.path()),
        temp.path(),
        false,
        memory,
        ModelSwitchConfig::default(),
    )
    .unwrap();
    app.mode = Mode::Streaming;
    app.turn_active = true;
    app.input.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('h'),
        crossterm::event::KeyModifiers::NONE,
    ));
    app.input.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('i'),
        crossterm::event::KeyModifiers::NONE,
    ));

    app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    )))
    .await
    .unwrap();

    assert_eq!(app.input.text(), "hi");
    assert_eq!(app.chat.len(), 0);
    assert_eq!(app.mode, Mode::Streaming);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p telos-cli tui::app::tests::streaming_character_input_updates_composer tui::app::tests::streaming_enter_keeps_draft_and_does_not_dispatch_prompt
```

Expected: FAIL because `Mode::Streaming` currently ignores character input and Enter toggles tool activity instead of preserving a draft.

- [ ] **Step 3: Prevent dispatch while a turn is active**

In `cli/src/tui/app/commands.rs`, change `handle_input_event` submit handling to:

```rust
InputEvent::Submit(prompt) => {
    if self.turn_active {
        self.input.restore_text(prompt);
        return;
    }
    self.send_prompt(prompt).await;
}
```

Add this method to `InputPanel` in `cli/src/tui/widgets/input_panel.rs`:

```rust
pub fn restore_text(&mut self, text: String) {
    self.set_text(&text);
}
```

- [ ] **Step 4: Route streaming input to composer**

In `cli/src/tui/app/events.rs`, update the `Mode::Streaming` arm so plain text keys reach the input panel after the existing scroll and tool activity shortcuts:

```rust
let input_event = self.input.handle_key(key);
self.handle_input_event(input_event).await;
```

Keep existing scroll handling returns for PageUp, PageDown, plain Up, plain Down, Tab, BackTab, and Ctrl+T. Do not include plain Enter in the tool expansion shortcut list; Enter should go through the input panel so the submit guard can restore the draft.

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test -p telos-cli tui::app::tests::streaming_character_input_updates_composer tui::app::tests::streaming_enter_keeps_draft_and_does_not_dispatch_prompt
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add cli/src/tui/app/events.rs cli/src/tui/app/commands.rs cli/src/tui/widgets/input_panel.rs cli/src/tui/app/mod.rs
git commit -m "feat: allow drafting during streaming"
```

## Task 5: Inline Approval Panel Rendering

**Files:**
- Create: `cli/src/tui/widgets/approval_inline.rs`
- Modify: `cli/src/tui/mod.rs`
- Modify: `cli/src/tui/app/mod.rs`

- [ ] **Step 1: Write the failing widget tests**

Create `cli/src/tui/widgets/approval_inline.rs` with the tests first:

```rust
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::approval::PendingApproval;
use crate::tui::theme::Theme;

pub const INLINE_APPROVAL_HEIGHT: u16 = 4;

pub fn render(_frame: &mut Frame, _area: Rect, _theme: &Theme, _pending: &PendingApproval) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use telos_agent::{ApprovalRequest, Message};
    use tokio::sync::oneshot;

    fn pending(tool_name: &str, arguments: serde_json::Value) -> PendingApproval {
        let (tx, _rx) = oneshot::channel();
        PendingApproval {
            request: ApprovalRequest {
                tool_name: tool_name.into(),
                invocation_names: vec![tool_name.into()],
                arguments,
                cwd: PathBuf::from("."),
                messages: Arc::new(vec![Message::user("hi")]),
                reason: "needs review".into(),
            },
            respond: Some(tx),
        }
    }

    #[test]
    fn lines_include_shell_command_and_actions() {
        let lines = approval_lines(&pending("Bash", json!({ "command": "rm target" })), 80);
        let text = lines.join("\n");

        assert!(text.contains("Approval required"));
        assert!(text.contains("Bash"));
        assert!(text.contains("rm target"));
        assert!(text.contains("y/a approve"));
        assert!(text.contains("n/d deny"));
    }

    #[test]
    fn lines_include_reason() {
        let lines = approval_lines(&pending("Write", json!({ "file_path": "src/main.rs", "content": "fn main() {}" })), 80);
        let text = lines.join("\n");

        assert!(text.contains("needs review"));
        assert!(text.contains("src/main.rs"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p telos-cli tui::approval_inline::tests::lines_include_shell_command_and_actions tui::approval_inline::tests::lines_include_reason
```

Expected: FAIL with compiler errors for missing `approval_lines` and missing module export.

- [ ] **Step 3: Export module**

Add to `cli/src/tui/mod.rs`:

```rust
#[path = "widgets/approval_inline.rs"]
pub mod approval_inline;
```

- [ ] **Step 4: Implement line generation and rendering**

Replace `cli/src/tui/widgets/approval_inline.rs` with:

```rust
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::approval::PendingApproval;
use crate::tui::overlay::truncate_for_popup;
use crate::tui::theme::Theme;

pub const INLINE_APPROVAL_HEIGHT: u16 = 4;

pub fn approval_lines(pending: &PendingApproval, width: usize) -> Vec<String> {
    let request = &pending.request;
    let tool = request.tool_name.trim();
    let detail_width = width.saturating_sub(18).max(24);
    let tool_lower = tool.to_lowercase();
    let detail = if tool_lower == "bash" || tool_lower == "shell" {
        request
            .arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(|command| format!("$ {}", truncate_for_popup(command, detail_width)))
            .unwrap_or_else(|| request.arguments.to_string())
    } else if tool_lower == "edit" {
        let file = request
            .arguments
            .get("file_path")
            .and_then(|value| value.as_str())
            .unwrap_or("?");
        format!("edit {}", truncate_for_popup(file, detail_width))
    } else if tool_lower == "write" {
        let file = request
            .arguments
            .get("file_path")
            .and_then(|value| value.as_str())
            .unwrap_or("?");
        format!("write {}", truncate_for_popup(file, detail_width))
    } else {
        truncate_for_popup(&request.arguments.to_string(), detail_width)
    };

    let reason = request.reason.trim();
    let reason = if reason.is_empty() {
        "review required".to_string()
    } else {
        truncate_for_popup(reason, detail_width)
    };

    vec![
        format!("Approval required · {tool}"),
        detail,
        reason,
        "y/a approve  n/d deny  e edit".to_string(),
    ]
}

pub fn render(frame: &mut Frame, area: Rect, theme: &Theme, pending: &PendingApproval) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = approval_lines(pending, area.width as usize)
        .into_iter()
        .enumerate()
        .map(|(idx, text)| {
            let style = match idx {
                0 => Style::default()
                    .fg(theme.tool_pending_fg)
                    .add_modifier(Modifier::BOLD),
                3 => Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
                _ => Style::default().fg(theme.assistant_fg),
            };
            Line::from(Span::styled(text, style))
        })
        .collect::<Vec<_>>();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.tool_pending_fg));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use telos_agent::{ApprovalRequest, Message};
    use tokio::sync::oneshot;

    fn pending(tool_name: &str, arguments: serde_json::Value) -> PendingApproval {
        let (tx, _rx) = oneshot::channel();
        PendingApproval {
            request: ApprovalRequest {
                tool_name: tool_name.into(),
                invocation_names: vec![tool_name.into()],
                arguments,
                cwd: PathBuf::from("."),
                messages: Arc::new(vec![Message::user("hi")]),
                reason: "needs review".into(),
            },
            respond: Some(tx),
        }
    }

    #[test]
    fn lines_include_shell_command_and_actions() {
        let lines = approval_lines(&pending("Bash", json!({ "command": "rm target" })), 80);
        let text = lines.join("\n");

        assert!(text.contains("Approval required"));
        assert!(text.contains("Bash"));
        assert!(text.contains("rm target"));
        assert!(text.contains("y/a approve"));
        assert!(text.contains("n/d deny"));
    }

    #[test]
    fn lines_include_reason() {
        let lines = approval_lines(
            &pending(
                "Write",
                json!({ "file_path": "src/main.rs", "content": "fn main() {}" }),
            ),
            80,
        );
        let text = lines.join("\n");

        assert!(text.contains("needs review"));
        assert!(text.contains("src/main.rs"));
    }
}
```

- [ ] **Step 5: Render panel in layout**

In `cli/src/tui/app/mod.rs`, import:

```rust
use crate::tui::approval_inline;
```

In `App::draw`, compute:

```rust
let approval_height =
    if self.inline_approval.is_some() { approval_inline::INLINE_APPROVAL_HEIGHT } else { 0 };
```

Add `Constraint::Length(approval_height)` between tool activity and input constraints, then render:

```rust
if let Some(pending) = &self.inline_approval {
    approval_inline::render(frame, layout[2], &theme, pending);
}
self.input.render(frame, layout[3], self.mode != Mode::Approving);
```

Move status rendering to `layout[4]`.

- [ ] **Step 6: Run tests to verify they pass**

Run:

```bash
cargo test -p telos-cli tui::approval_inline::tests::lines_include_shell_command_and_actions tui::approval_inline::tests::lines_include_reason
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add cli/src/tui/widgets/approval_inline.rs cli/src/tui/mod.rs cli/src/tui/app/mod.rs
git commit -m "feat: render inline approval panel"
```

## Task 6: Regression Suite And Cleanup

**Files:**
- Modify as needed: `cli/src/tui/app/events.rs`
- Modify as needed: `cli/src/tui/overlays/overlay.rs`
- Modify as needed: `cli/src/tui/app/mod.rs`

- [ ] **Step 1: Remove stale approval overlay routing**

Search:

```bash
rg -n "ApprovalOverlay|enqueue_inline_approval|inline_approval|Mode::Approving" cli/src/tui
```

Expected: `ApprovalOverlay` may remain defined for tests or future fallback, but `app/events.rs` should no longer construct it for approval channel events. `Mode::Approving` should remain for selection and edit popups.

- [ ] **Step 2: Run focused TUI tests**

Run:

```bash
cargo test -p telos-cli tui::input_panel::tests tui::app::tests tui::approval_inline::tests
```

Expected: PASS.

- [ ] **Step 3: Run full CLI crate tests**

Run:

```bash
cargo test -p telos-cli
```

Expected: PASS.

- [ ] **Step 4: Run formatting check**

Run:

```bash
cargo fmt --check
```

Expected: PASS with no diff required.

- [ ] **Step 5: Commit verification cleanup**

If cleanup or formatting changed files:

```bash
git add cli/src/tui
git commit -m "test: cover inline approval regressions"
```

If no files changed, skip this commit.

## Self-Review

- Spec coverage: Tasks 2 and 3 move approval state from overlay to inline app state; Task 5 renders the card; Task 4 allows drafting during streaming and blocks concurrent send; Task 2 covers FIFO pending approvals and approval resolution.
- Placeholder scan: No task uses TBD, TODO, or open-ended implementation instructions. Each code-changing step includes concrete paths, code, and commands.
- Type consistency: The plan consistently uses `inline_approval: Option<PendingApproval>`, `inline_approval_queue: VecDeque<PendingApproval>`, `enqueue_inline_approval`, `resolve_inline_approval`, and `InputPanel::text`.
