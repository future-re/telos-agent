"""Central reactive state store. Widgets watch these fields."""

from dataclasses import dataclass, field
from typing import Optional

from textual.dom import DOMNode
from textual.reactive import reactive


@dataclass
class ToolEntry:
    """Tracks a single tool call through its lifecycle."""
    call_id: str
    name: str
    detail: str = ""
    status: str = "running"  # running | ok | error
    result_lines: list[str] = field(default_factory=list)


@dataclass
class Message:
    """A single message in the chat."""
    role: str  # "user" | "assistant" | "thinking" | "system" | "tool"
    text: str = ""
    tool_call_id: str = ""
    tool_name: str = ""
    tool_detail: str = ""
    tool_status: str = ""
    is_error: bool = False


class AppState(DOMNode):
    """Reactive store. App owns one instance. Widgets watch fields."""

    def __init__(self) -> None:
        super().__init__()

    # Connection
    connected: reactive[bool] = reactive(False)

    # Streaming state
    streaming: reactive[bool] = reactive(False)

    # Messages — the single source of truth for chat content
    messages: reactive[list[Message]] = reactive([])

    # Tool entries (sidebar / status reference)
    tool_entries: reactive[list[ToolEntry]] = reactive([])

    # Approval
    pending_approval: reactive[Optional[dict]] = reactive(None)

    # Token usage
    input_tokens: reactive[int] = reactive(0)
    output_tokens: reactive[int] = reactive(0)
    token_budget_max: reactive[int] = reactive(0)
    cost: reactive[float] = reactive(0.0)

    # Turn tracking
    turn_elapsed: reactive[float] = reactive(0.0)

    # Auto mode
    auto_approve: reactive[bool] = reactive(False)

    # Status line text
    status_text: reactive[str] = reactive("telos · starting…")

    # ── helpers ──────────────────────────────────────────────────

    def add_message(self, msg: Message) -> None:
        """Append a message and trigger reactive update."""
        self.messages = self.messages + [msg]

    def update_last_assistant(self, text: str) -> None:
        """Update the text of the last assistant message (streaming)."""
        msgs = list(self.messages)
        if msgs and msgs[-1].role == "assistant":
            msgs[-1] = Message(role="assistant", text=text)
            self.messages = msgs  # type: ignore[assignment]

    def update_last_thinking(self, text: str) -> None:
        """Update the text of the last thinking message (streaming)."""
        msgs = list(self.messages)
        if msgs and msgs[-1].role == "thinking":
            msgs[-1] = Message(role="thinking", text=text)
            self.messages = msgs  # type: ignore[assignment]

    def upsert_tool(self, call_id: str, name: str = "", detail: str = "",
                    status: str = "") -> None:
        """Insert or update a tool entry."""
        entries = list(self.tool_entries)
        for e in entries:
            if e.call_id == call_id:
                if name:
                    e.name = name
                if detail:
                    e.detail = detail
                if status:
                    e.status = status
                self.tool_entries = entries  # type: ignore[assignment]
                return
        entries.append(ToolEntry(call_id=call_id, name=name, detail=detail,
                                 status=status or "running"))
        self.tool_entries = entries  # type: ignore[assignment]

    def tool_result(self, call_id: str, result: str, is_error: bool = False) -> None:
        """Append result lines to a tool entry."""
        entries = list(self.tool_entries)
        for e in entries:
            if e.call_id == call_id:
                e.result_lines = result.splitlines()[:8]
                if is_error:
                    e.status = "error"
                self.tool_entries = entries  # type: ignore[assignment]
                return

    def clear(self) -> None:
        """Reset state for a new session."""
        self.messages = []  # type: ignore[assignment]
        self.tool_entries = []  # type: ignore[assignment]
        self.pending_approval = None
        self.streaming = False
        self.status_text = "telos · new session"
