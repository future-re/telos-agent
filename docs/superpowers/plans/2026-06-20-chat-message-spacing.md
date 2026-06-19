# Chat Message Spacing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make user prompts and assistant replies in the TUI transcript less visually dense and easier to scan.

**Architecture:** Keep rendering responsibility inside existing `HistoryCell` implementations. Add focused `TestBackend` coverage for user/assistant spacing and avoid changing `ChatWidget` scrolling or app-level layout.

**Tech Stack:** Rust 2024, Ratatui `TestBackend`, existing TUI history cell system

---

## File Structure

- Modify `cli/src/tui/history_cell.rs`: add small helpers for user block lines and assistant spacing, update `UserCell` and `AgentCell` measurement/rendering.
- Modify `cli/src/tui/chat_widget.rs`: add a focused rendering test for a user prompt followed by an assistant reply.
- Do not modify `cli/src/tui/app.rs`, input handling, core, providers, or session persistence.

---

### Task 1: Add Failing Spacing Test

**Files:**
- Modify: `cli/src/tui/chat_widget.rs`

- [ ] **Step 1: Add a regression test for user/assistant spacing**

In `cli/src/tui/chat_widget.rs`, inside the existing `#[cfg(test)] mod tests`, add this test after `short_history_renders_against_bottom_edge`:

```rust
#[test]
fn user_and_assistant_blocks_have_breathing_room() {
    let mut chat = ChatWidget::new();
    chat.push_cell(Box::new(UserCell { content: "make it readable".to_string() }));
    chat.push_cell(Box::new(AgentCell {
        buffer: "Done.\n\nI adjusted the spacing.".to_string(),
        is_streaming: false,
    }));

    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::default();
    terminal.draw(|frame| chat.render(frame, frame.area(), &theme)).unwrap();

    assert!(rendered_row(&terminal, 1).trim().is_empty());
    assert!(rendered_row(&terminal, 2).contains("▸ make it readable"));
    assert!(rendered_row(&terminal, 3).trim().is_empty());
    assert!(rendered_row(&terminal, 4).contains("Done."));
    assert!(rendered_row(&terminal, 6).contains("I adjusted the spacing."));
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run:

```bash
cargo test -p telos-cli tui::chat_widget::tests::user_and_assistant_blocks_have_breathing_room --lib
```

Expected: FAIL because assistant blocks do not yet reserve the intended separator line.

---

### Task 2: Implement Message Spacing

**Files:**
- Modify: `cli/src/tui/history_cell.rs`

- [ ] **Step 1: Add a helper for user lines**

In `cli/src/tui/history_cell.rs`, add this private helper above `impl HistoryCell for UserCell`:

```rust
fn user_lines(content: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = vec![Line::from("")];
    lines.extend(content.lines().enumerate().map(|(idx, line)| {
        let marker = if idx == 0 { "▸ " } else { "  " };
        Line::from(vec![
            Span::styled(marker.to_string(), theme.user_style()),
            Span::styled(line.to_string(), theme.user_style()),
        ])
    }));
    lines
}
```

- [ ] **Step 2: Use `user_lines` in `UserCell::render` and `render_scrolled`**

Replace both duplicated user-line construction blocks with:

```rust
let text = Text::from(user_lines(&self.content, theme));
```

Keep the existing `Paragraph::new(text).wrap(Wrap { trim: true })` rendering behavior.

- [ ] **Step 3: Add assistant separator measurement**

In `AgentCell::needed_lines`, change the non-empty return from:

```rust
rendered.lines.len() as u16
```

to:

```rust
rendered.lines.len() as u16 + 1
```

- [ ] **Step 4: Add assistant separator rendering**

In `AgentCell::render`, reserve the first line as spacing:

```rust
let content_area = Rect {
    x: area.x,
    y: area.y.saturating_add(1),
    width: area.width,
    height: area.height.saturating_sub(1),
};
if content_area.height == 0 {
    return;
}
```

Render diff/markdown into `content_area` instead of `area`.

- [ ] **Step 5: Add assistant separator scrolled rendering**

In `AgentCell::render_scrolled`, represent the separator as line zero:

```rust
if top_skip == 0 {
    let content_area = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    if content_area.height == 0 {
        return;
    }
    // render with scroll offset 0 into content_area
} else {
    let adjusted_skip = top_skip.saturating_sub(1);
    // render into area with adjusted_skip
}
```

- [ ] **Step 6: Run the focused test**

Run:

```bash
cargo test -p telos-cli tui::chat_widget::tests::user_and_assistant_blocks_have_breathing_room --lib
```

Expected: PASS.

---

### Task 3: Verify and Commit

**Files:**
- Modify: `cli/src/tui/history_cell.rs`
- Modify: `cli/src/tui/chat_widget.rs`

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: exits successfully. Existing nightly-only rustfmt warnings may print.

- [ ] **Step 2: Run TUI tests**

Run:

```bash
cargo test -p telos-cli tui::chat_widget --lib
```

Expected: PASS.

- [ ] **Step 3: Run CLI library tests**

Run:

```bash
cargo test -p telos-cli --lib
```

Expected: PASS.

- [ ] **Step 4: Commit only the TUI files**

Run:

```bash
git add cli/src/tui/history_cell.rs cli/src/tui/chat_widget.rs
git commit -m "fix(tui): loosen chat message spacing"
```

Expected: commit succeeds and includes only those two files.

---

## Self-Review

- Spec coverage: user block separator, assistant breathing room, markdown preservation, bottom-alignment preservation, and local TUI tests are covered.
- Placeholder scan: no TODO/TBD/fill-in steps remain.
- Type consistency: all referenced files and test names exist in the current TUI module structure.
