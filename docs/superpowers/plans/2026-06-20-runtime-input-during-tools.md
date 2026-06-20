# Runtime Input During Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add live user input during an active turn, injected after tool results with a forced pro-model rethink.

**Architecture:** Add a per-turn input channel in `core/src/runtime`, expose a stream variant that drains it after tool execution, and update the CLI background loop to forward running prompts into that channel while still streaming turn events.

**Tech Stack:** Rust, Tokio `mpsc`, futures streams, existing `ModelHint` routing, existing TUI app/background channels.

---

### Task 1: Core Runtime Input Channel

**Files:**
- Create: `core/src/runtime/input.rs`
- Modify: `core/src/runtime/mod.rs`
- Modify: `core/src/runtime/session.rs`
- Test: `core/tests/runtime_tests.rs`

- [ ] Write a failing runtime test that sends input while a slow tool is running and asserts the second provider request uses `ModelHint::Thinking`.
- [ ] Add `TurnInputSender`, `TurnInputReceiver`, and `turn_input_channel`.
- [ ] Add `AgentSession::run_turn_stream_with_input`.
- [ ] Drain pending input after tool results are pushed and emit `TurnEvent::User`.
- [ ] Force the next provider hint to `ModelHint::Thinking`.
- [ ] Run `cargo test -p telos-agent --test runtime_tests runtime_input_after_tool_forces_thinking_reconsideration`.

### Task 2: CLI Streaming Prompt Forwarding

**Files:**
- Modify: `cli/src/tui/app/background.rs`
- Modify: `cli/src/tui/app/commands.rs`
- Modify: `cli/src/tui/app/mod.rs`

- [ ] Write or update a TUI test showing `InputEvent::Submit` while `turn_active` is accepted.
- [ ] Update `handle_input_event` to call `send_prompt` even when streaming.
- [ ] Update background turn execution to select over stream events and command input.
- [ ] Forward `Prompt` commands during an active turn to `TurnInputSender`.
- [ ] Defer non-prompt commands until after the active turn.
- [ ] Run focused CLI tests.

### Task 3: Verification

**Files:**
- All modified files

- [ ] Run `cargo fmt --all`.
- [ ] Run `cargo test -p telos-agent --test runtime_tests`.
- [ ] Run focused CLI tests for TUI app behavior.
- [ ] Run `git diff --check`.
