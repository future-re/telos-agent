"""Event dispatch — translates telos serve events into AppState updates."""

from .state import AppState, Message
from .streaming import StreamBuffer


class EventLoop:
    """Receives raw JSON events from Protocol and updates AppState.

    Does NOT touch widgets directly — widgets watch AppState reactively.
    """

    def __init__(self, state: AppState) -> None:
        self.state = state
        self._assistant_buf = StreamBuffer()
        self._thinking_buf = StreamBuffer()

    def handle_event(self, event: dict) -> None:
        event_type = event.get("type", "")

        if event_type == "TurnStarted":
            self._handle_turn_started(event)
        elif event_type == "TurnFinished":
            self._handle_turn_finished(event)
        elif event_type == "AssistantDelta":
            self._handle_assistant_delta(event)
        elif event_type == "ThinkingDelta":
            self._handle_thinking_delta(event)
        elif event_type == "Assistant":
            self._handle_assistant(event)
        elif event_type == "User":
            pass  # already shown when sent
        elif event_type == "ToolCall":
            self._handle_tool_call(event)
        elif event_type == "ToolProgress":
            self._handle_tool_progress(event)
        elif event_type == "ToolCompleted":
            self._handle_tool_completed(event)
        elif event_type == "ToolResult":
            self._handle_tool_result(event)
        elif event_type == "ProviderUsage":
            self._handle_provider_usage(event)
        elif event_type == "ApprovalRequested":
            self._flush_assistant()
            name = event.get("name", "")
            self.state.add_message(Message(role="system",
                                           text=f"Awaiting approval: {name}"))
        elif event_type == "ApprovalResolved":
            name = event.get("name", "")
            decision = event.get("decision", "")
            self.state.add_message(Message(
                role="system",
                text=f"Approval resolved: {name} -> {decision}"
            ))
        elif event_type == "_approval_required":
            self._flush_assistant()
            self.state.pending_approval = event
            self.state.status_text = "telos · approval required"
        elif event_type == "_done":
            self._flush_assistant()
            self.state.streaming = False
            self.state.status_text = "telos · ready"
        elif event_type == "_error":
            self._flush_thinking()
            msg = event.get("message", "unknown error")
            self.state.add_message(Message(role="system", text=f"Error: {msg}"))
        elif event_type == "_session_new":
            self.state.clear()
            self.state.add_message(Message(role="system",
                                           text="New session started."))
        elif event_type == "CompactionStarted":
            self.state.status_text = "telos · compacting…"
        elif event_type == "CompactionCompleted":
            self.state.status_text = "telos · ready"

    # ── handlers ─────────────────────────────────────────────────

    def _handle_turn_started(self, event: dict) -> None:
        self._assistant_buf.reset()
        self._thinking_buf.reset()
        self.state.streaming = True
        self.state.status_text = "telos · thinking…"
        self.state.tool_entries = []  # type: ignore[assignment]

    def _handle_turn_finished(self, event: dict) -> None:
        self._flush_assistant()
        self.state.streaming = False
        self.state.status_text = "telos · ready"

    def _handle_assistant_delta(self, event: dict) -> None:
        text = event.get("text", "")
        msgs = self.state.messages
        if not msgs or msgs[-1].role != "assistant":
            self._flush_assistant()
            self.state.add_message(Message(role="assistant", text=""))
        full = self._assistant_buf.feed(text)
        if full is not None:
            self.state.update_last_assistant(full)

    def _handle_thinking_delta(self, event: dict) -> None:
        text = event.get("text", "")
        msgs = self.state.messages
        if not msgs or msgs[-1].role != "thinking":
            self._flush_thinking()
            self.state.add_message(Message(role="thinking", text=""))
        full = self._thinking_buf.feed(text)
        if full is not None:
            self.state.update_last_thinking(full)

    def _handle_assistant(self, event: dict) -> None:
        self._flush_thinking()
        text = event.get("text", "")
        if text:
            self.state.add_message(Message(role="assistant", text=text))

    def _handle_tool_call(self, event: dict) -> None:
        call_id = event.get("call_id", "")
        name = event.get("name", "?")
        detail = event.get("detail", "")
        self._flush_assistant()
        self.state.upsert_tool(call_id=call_id, name=name, detail=detail,
                               status="running")
        self.state.add_message(Message(role="tool", tool_call_id=call_id,
                                       tool_name=name, tool_detail=detail,
                                       tool_status="running"))

    def _handle_tool_progress(self, event: dict) -> None:
        call_id = event.get("call_id", "")
        msg = event.get("message", "")
        if msg and call_id:
            entries = list(self.state.tool_entries)
            for e in entries:
                if e.call_id == call_id:
                    e.result_lines.append(msg)
                    self.state.tool_entries = entries  # type: ignore[assignment]
                    break

    def _handle_tool_completed(self, event: dict) -> None:
        call_id = event.get("call_id", "")
        name = event.get("name", "?")
        is_error = event.get("is_error", False)
        status = "error" if is_error else "ok"
        self.state.upsert_tool(call_id=call_id, name=name, status=status)
        msgs = list(self.state.messages)
        for i, m in enumerate(msgs):
            if m.role == "tool" and m.tool_call_id == call_id:
                msgs[i] = Message(role="tool", tool_call_id=call_id,
                                  tool_name=m.tool_name,
                                  tool_detail=m.tool_detail,
                                  tool_status=status, is_error=is_error,
                                  text=m.text)
                self.state.messages = msgs  # type: ignore[assignment]
                break

    def _handle_tool_result(self, event: dict) -> None:
        call_id = event.get("call_id", "")
        result = event.get("result", "")
        is_error = event.get("is_error", False)
        self.state.tool_result(call_id, result, is_error)

    def _handle_provider_usage(self, event: dict) -> None:
        self.state.input_tokens = event.get("input_tokens", 0)
        self.state.output_tokens = event.get("output_tokens", 0)
        total = event.get("total_tokens") or (
            self.state.input_tokens + self.state.output_tokens
        )
        self.state.cost = event.get("cost", 0.0)
        self.state.token_budget_max = event.get("token_budget_max", 0)
        up_k = self.state.input_tokens / 1000
        down_k = self.state.output_tokens / 1000
        parts = [f"telos · up {up_k:.1f}k down {down_k:.1f}k"]
        if self.state.cost > 0:
            parts.append(f"${self.state.cost:.4f}")
        self.state.status_text = " · ".join(parts)

    def _flush_assistant(self) -> None:
        self._flush_thinking()
        full = self._assistant_buf.flush()
        if full is not None:
            self.state.update_last_assistant(full)
        self._assistant_buf.reset()

    def _flush_thinking(self) -> None:
        full = self._thinking_buf.flush()
        if full is not None:
            self.state.update_last_thinking(full)
        self._thinking_buf.reset()
