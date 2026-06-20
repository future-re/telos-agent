# Runtime Input During Tools Design

## Goal

Allow a user to submit additional input while an agent turn is running. If that input arrives before or during tool execution, the runtime must append it after tool results and force the next provider call to use `ModelHint::Thinking`, which routes to the pro model in the current DeepSeek routing setup.

## Architecture

The core runtime gets a small optional input queue for a single turn. Hosts that do not need live input keep using `AgentSession::run_turn_stream`; interactive hosts use a new stream variant with a `TurnInputReceiver`.

The runtime never interrupts an in-flight provider request or tool call. It checks the queue after tool results are appended to the conversation. Drained inputs become user messages in the same turn, and the following provider call is forced to `ModelHint::Thinking`.

## CLI Behavior

The TUI background task must read UI commands while a turn stream is active. A `Prompt` command received during a running turn is sent to the active turn input queue instead of waiting for the outer command loop. The chat UI shows the user message immediately when submitted.

Non-prompt commands received during an active turn remain deferred until the turn finishes.

## Testing

Core tests verify that a prompt submitted while a tool is running appears after the tool result in the same turn and that the next provider request uses `ModelHint::Thinking`.

CLI tests verify that submitting while `turn_active` does not restore text or reject the prompt.
