# Input Composer Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish the TUI input panel into the approved Codex-style composer without changing input behavior.

**Architecture:** Keep `InputPanel` as the owner of input state and rendering. Extract only the footer hint layout into a small pure helper so narrow-width behavior can be tested without brittle terminal snapshots.

**Tech Stack:** Rust 2024, Ratatui, Crossterm, tui-textarea

---

## File Structure

- Modify `cli/src/tui/input_panel.rs`: add a small `ComposerHints` helper, unit tests for width-aware footer text, and update `InputPanel::new` / `InputPanel::render` copy and layout.
- No theme changes unless implementation shows repeated hard-coded colors; existing `Theme` fields are enough for the approved B direction.
- No changes to keyboard handling functions. `handle_normal_key`, `handle_slash_key`, and `handle_paste_key` should remain behaviorally unchanged.

---

### Task 1: Footer Hint Layout Helper

**Files:**
- Modify: `cli/src/tui/input_panel.rs`

- [ ] **Step 1: Add failing tests for composer footer hint layout**

Add this test module near the end of `cli/src/tui/input_panel.rs`, before `impl Default for InputPanel`:

```rust
#[cfg(test)]
mod tests {
    use super::ComposerHints;

    #[test]
    fn composer_hints_split_when_width_allows() {
        let hints = ComposerHints::normal(96);

        assert_eq!(hints.left, " Enter send  Alt+Enter newline ");
        assert_eq!(hints.right.as_deref(), Some(" Ctrl+up/down history  Shift+Tab auto  Ctrl+D quit "));
    }

    #[test]
    fn composer_hints_collapse_on_narrow_width() {
        let hints = ComposerHints::normal(34);

        assert_eq!(hints.left, " Enter send  Alt+Enter newline ");
        assert_eq!(hints.right, None);
    }

    #[test]
    fn composer_hints_show_history_position() {
        let hints = ComposerHints::history(2, 5);

        assert_eq!(hints.left, " History 3/5 ");
        assert_eq!(hints.right, None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p telos-cli tui::input_panel --lib
```

Expected: FAIL because `ComposerHints` does not exist.

- [ ] **Step 3: Implement the minimal helper**

Add this helper above `pub struct InputPanel` in `cli/src/tui/input_panel.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposerHints {
    left: String,
    right: Option<String>,
}

impl ComposerHints {
    fn normal(width: u16) -> Self {
        let left = String::from(" Enter send  Alt+Enter newline ");
        let right = String::from(" Ctrl+up/down history  Shift+Tab auto  Ctrl+D quit ");

        if usize::from(width) >= left.len() + right.len() + 2 {
            Self { left, right: Some(right) }
        } else {
            Self { left, right: None }
        }
    }

    fn history(index: usize, len: usize) -> Self {
        Self { left: format!(" History {}/{} ", index + 1, len), right: None }
    }
}
```

- [ ] **Step 4: Run tests to verify helper passes**

Run:

```bash
cargo test -p telos-cli tui::input_panel --lib
```

Expected: PASS for the three new `input_panel` tests.

- [ ] **Step 5: Commit helper and tests**

```bash
git add cli/src/tui/input_panel.rs
git commit -m "test(tui): cover composer footer hints"
```

---

### Task 2: Apply Codex-Style Composer Rendering

**Files:**
- Modify: `cli/src/tui/input_panel.rs`

- [ ] **Step 1: Update placeholder text**

In `InputPanel::new`, replace the current placeholder:

```rust
textarea
    .set_placeholder_text("Message… (/ for commands, Enter to send, Alt+Enter newline)");
```

with:

```rust
textarea.set_placeholder_text("Ask tiny-agent to edit, inspect, or run...");
```

- [ ] **Step 2: Update active title copy**

In `InputPanel::render`, replace the non-paste active title arm:

```rust
_ => Span::styled(" Message ", border_style),
```

with:

```rust
_ => Span::styled(" Compose ", border_style),
```

Keep the paste title exactly as:

```rust
InputMode::Pasting { line_count } => {
    Span::styled(format!(" Pasted {line_count} lines — y(es)/n(o)? "), border_style)
}
```

- [ ] **Step 3: Add right-side mode hint to the block title**

Replace the block construction:

```rust
let block = Block::default()
    .borders(Borders::ALL)
    .border_style(border_style)
    .title_top(Line::from(title).left_aligned());
```

with:

```rust
let mut block = Block::default()
    .borders(Borders::ALL)
    .border_style(border_style)
    .title_top(Line::from(title).left_aligned());

if active && !matches!(self.mode, InputMode::Pasting { .. }) {
    block = block.title_top(
        Line::from(Span::styled(
            " / commands ",
            Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
        ))
        .right_aligned(),
    );
}
```

- [ ] **Step 4: Change prompt marker and width**

Replace:

```rust
let prompt_w = 2u16;
let prompt_area = Rect { x: inner.x, y: inner.y, width: prompt_w, height: 1 };
let prompt = Paragraph::new(Line::from(Span::styled(
    "> ",
    Style::default().fg(theme.user_fg).add_modifier(Modifier::BOLD),
)));
```

with:

```rust
let prompt_w = 3u16;
let prompt_area = Rect { x: inner.x, y: inner.y, width: prompt_w, height: 1 };
let prompt = Paragraph::new(Line::from(Span::styled(
    "› ",
    Style::default().fg(theme.user_fg).add_modifier(Modifier::BOLD),
)));
```

- [ ] **Step 5: Render split footer hints**

Replace the existing hint construction and render block:

```rust
let hint = match self.history_pos {
    Some(i) => format!(" history [{}/{}] ", i + 1, self.history.len()),
    None => String::from(
        " enter·send  alt+enter·newline  ctrl+↑↓·history  shift+tab·auto  ctrl+d·quit ",
    ),
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
```

with:

```rust
let hints = match self.history_pos {
    Some(i) => ComposerHints::history(i, self.history.len()),
    None => ComposerHints::normal(inner.width),
};
let hint_style = Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM);
let footer_y = inner.y + inner.height.saturating_sub(1);

frame.render_widget(
    Paragraph::new(hints.left).style(hint_style),
    Rect { y: footer_y, x: inner.x, width: inner.width, height: 1 },
);

if let Some(right) = hints.right {
    let right_width = right.len().min(usize::from(inner.width)) as u16;
    frame.render_widget(
        Paragraph::new(right).style(hint_style),
        Rect {
            y: footer_y,
            x: inner.x + inner.width.saturating_sub(right_width),
            width: right_width,
            height: 1,
        },
    );
}
```

- [ ] **Step 6: Run targeted tests**

Run:

```bash
cargo test -p telos-cli tui::input_panel --lib
```

Expected: PASS.

- [ ] **Step 7: Run formatting**

Run:

```bash
cargo fmt
```

Expected: command exits successfully and only formats touched Rust files if needed.

- [ ] **Step 8: Run CLI test suite**

Run:

```bash
cargo test -p telos-cli --lib
```

Expected: PASS. If unrelated tests fail, capture the failing test names and error output before changing anything.

- [ ] **Step 9: Commit rendering polish**

```bash
git add cli/src/tui/input_panel.rs
git commit -m "feat(tui): polish input composer"
```

---

## Self-Review

- Spec coverage: The plan updates placeholder copy, active title, right-side `/ commands` hint, prompt marker, footer grouping, paste/history preservation, and narrow-width footer behavior.
- Placeholder scan: No TODO, TBD, or undefined implementation steps remain.
- Type consistency: `ComposerHints::normal(width: u16)` and `ComposerHints::history(index, len)` are used consistently in tests and render code.
