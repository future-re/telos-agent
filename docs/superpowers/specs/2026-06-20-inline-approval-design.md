# Inline Approval Design

## Goal

Replace the current centered TUI approval popup with an inline approval card near the composer. The approval experience should feel native to the main chat surface and should not block the user from drafting input while the agent is running.

## Current Behavior

The TUI receives tool approval requests through `PendingApproval` from `TuiApprovalHandler`. `App::handle_event` pushes an `ApprovalOverlay` and switches to `Mode::Approving`. While this overlay is active, key input is routed only to the overlay. During normal streaming, `Mode::Streaming` also ignores text input and only handles scrolling and tool expansion. This makes approval visually disruptive and prevents composing text while the agent is executing.

## Proposed Behavior

Use a bottom inline approval panel instead of the centered popup.

When a pending approval arrives, `App` stores it as active inline approval state instead of pushing `ApprovalOverlay`. The main layout renders a compact approval card above the composer and below live tool activity. The card shows:

- Tool name.
- Short tool-specific preview, such as shell command or file path plus edit preview.
- Approval reason when available.
- Keyboard actions: `y` or `a` to approve, `n` or `d` to deny, `e` to edit, and `r` to toggle remember if retained.

The app no longer switches into a separate approving mode for the normal approve or deny path. While a turn is streaming, ordinary text input continues to update the composer. Approval shortcut keys are handled first only when an inline approval is active.

## Components

`App` owns the active pending approval state. It is responsible for sending `ApprovalDecision` through the existing oneshot responder and clearing the inline approval after resolution.

`ApprovalInlinePanel` is a new TUI widget or helper module. It reuses the display formatting concepts from the existing approval overlay, but renders within an allocated layout row rather than over the full screen. It should keep previews compact and wrap safely within the terminal width.

The existing `TuiApprovalHandler` and core approval API stay unchanged.

The existing edit flow can remain popup-based for this iteration. Pressing `e` opens the current approval edit popup so command editing behavior is not lost. A later change can make editing inline if needed.

## Event Flow

1. Background tool execution asks for approval through `TuiApprovalHandler`.
2. The handler sends `PendingApproval` to the app over the existing approval channel.
3. On tick, the app stores the pending request in inline approval state.
4. Draw allocates approval panel height only while inline approval state exists.
5. Key handling checks inline approval shortcuts before normal streaming/input handling.
6. Approve or deny sends the existing `ApprovalDecision`, clears inline approval, and returns focus to the composer.
7. Other text keys continue to edit the composer during streaming.

## Input During Streaming

`Mode::Streaming` should allow the input panel to handle text editing keys. Enter should not start a second concurrent turn while `turn_active` is true. The submitted text should remain in the composer or be ignored until a follow-up-turn queue is explicitly designed. The immediate requirement is drafting text during execution, not sending parallel turns.

Scrolling and tool activity shortcuts should continue to work. Existing scroll keys can keep their current precedence for plain Up and Down when the composer does not need vertical navigation.

## Error Handling

If the approval response channel is already closed, resolving the inline approval should clear it and add a session error or status notice rather than panic.

If a second approval arrives before the first is resolved, queue it in arrival order or keep the existing behavior equivalent to overlay stacking. The first implementation should use a small `VecDeque<PendingApproval>` so approval requests are handled one at a time without dropping later requests.

## Testing

Add unit tests around TUI app event handling:

- Receiving a pending approval stores inline approval state and does not switch to `Mode::Approving`.
- Pressing `y` or `a` resolves the approval with allow.
- Pressing `n` or `d` resolves the approval with deny.
- During `Mode::Streaming`, normal character input updates the composer when an approval is active or absent.
- Pressing Enter during streaming does not dispatch a second prompt.
- Multiple pending approvals are processed in order.

Add focused rendering tests if existing widget tests support ratatui buffers; otherwise keep render behavior covered by panel helper tests for line generation and truncation.
