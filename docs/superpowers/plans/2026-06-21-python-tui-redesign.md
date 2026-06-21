# Python Textual TUI Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-architect the Python Textual TUI from an app.py monolith with inline widgets into a modular, reactive-state-driven architecture with MessageBubble-based chat rendering and collapsible ToolCards.

**Architecture:** Central reactive AppState store → widgets watch state fields → events from `telos serve` protocol update state only. Chat rendered as a list of message/tool-card widgets rather than RichLog line-by-line writing. Streaming uses buffer + throttle.

**Tech Stack:** Python >= 3.9, Textual >= 0.60, Rich (bundled), telos serve (Rust backend, JSON-line protocol)

## Global Constraints

- Python >= 3.9
- Textual >= 0.60, <2
- Protocol unchanged: JSON-line over stdin/stdout to `telos serve`
- No new dependencies beyond textual+rich
- Widget classes in `telos_tui/widgets/`, core logic in `telos_tui/`
- Be compatible with Python 3.13

---

## Phase 1: State & Core Modules

### Task 1: Create state.py — AppState store

**Files:**
- Create: `cli/python/telos_tui/state.py`

**Interfaces:**
- Produces: `AppState` class with reactive fields; `ToolEntry` dataclass; `Message` dataclass for chat messages

- [ ] **Step 1: Create state.py**

```python
"""Central reactive state store. Widgets watch these fields."""

from dataclasses import dataclass, field
from typing import Optional

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


class AppState:
    """Reactive store. App owns one instance. Widgets watch fields."""

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
```

- [ ] **Step 2: Verify imports**

Run: `cd cli/python && python -c "from telos_tui.state import AppState, Message, ToolEntry; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/state.py
git commit -m "feat: add AppState reactive store

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Create streaming.py — StreamBuffer

**Files:**
- Create: `cli/python/telos_tui/streaming.py`

**Interfaces:**
- Produces: `StreamBuffer` class — `feed(delta: str) -> Optional[str]`, `flush() -> Optional[str]`
- Consumed by: event_loop.py, app.py for rendering assistant streaming text

- [ ] **Step 1: Create streaming.py**

```python
"""Streaming text buffer with paragraph-boundary and throttle rendering."""

import time


class StreamBuffer:
    """Accumulates deltas and yields full text at sensible boundaries.

    Renders on paragraph breaks (double newline) or when enough time
    has passed since the last render.  Avoids re-rendering markdown on
    every single-token delta.
    """

    def __init__(self, throttle_ms: int = 50) -> None:
        self._buffer: list[str] = []
        self._throttle_ms = throttle_ms
        self._last_render = 0.0
        self._rendered_len = 0

    def feed(self, delta: str) -> str | None:
        """Feed a delta. Returns the full text if it's time to render, else None."""
        self._buffer.append(delta)
        now = time.monotonic()
        elapsed_ms = (now - self._last_render) * 1000

        full = "".join(self._buffer)
        # Always render on a paragraph boundary
        if "\n\n" in full[self._rendered_len:]:
            self._rendered_len = len(full)
            self._last_render = now
            return full
        # Throttle to at most every throttle_ms
        if elapsed_ms >= self._throttle_ms and len(full) > self._rendered_len:
            self._rendered_len = len(full)
            self._last_render = now
            return full
        return None

    def flush(self) -> str | None:
        """Force-render whatever remains. Returns None if nothing new."""
        full = "".join(self._buffer)
        if len(full) > self._rendered_len:
            self._rendered_len = len(full)
            return full
        return None

    def reset(self) -> None:
        """Reset for a new streaming message."""
        self._buffer = []
        self._rendered_len = 0
        self._last_render = 0.0
```

- [ ] **Step 2: Verify basic behavior**

Run:
```bash
cd cli/python && python -c "
from telos_tui.streaming import StreamBuffer
b = StreamBuffer(throttle_ms=0)
assert b.feed('Hello') == 'Hello'
assert b.feed(' world') == 'Hello world'
assert b.feed('\n\nPara') == 'Hello world\n\nPara'
b.reset()
assert b.feed('') is None
b2 = StreamBuffer(throttle_ms=10000)
assert b2.feed('x') is None  # throttled
print('OK')
"
```
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/streaming.py
git commit -m "feat: add StreamBuffer for throttled markdown rendering

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Create event_loop.py — Event dispatch

**Files:**
- Create: `cli/python/telos_tui/event_loop.py`

**Interfaces:**
- Consumes: `AppState` from Task 1, `StreamBuffer` from Task 2, `ServeProtocol` (existing)
- Produces: `EventLoop` class with `handle_event(event: dict) -> None`

- [ ] **Step 1: Create event_loop.py**

```python
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
        short = text[:200] + "…" if len(text) > 200 else text
        self.state.add_message(Message(role="thinking", text=short))

    def _handle_assistant(self, event: dict) -> None:
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
        full = self._assistant_buf.flush()
        if full is not None:
            self.state.update_last_assistant(full)
        self._assistant_buf.reset()
```

- [ ] **Step 2: Verify imports**

Run: `cd cli/python && python -c "from telos_tui.event_loop import EventLoop; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/event_loop.py
git commit -m "feat: add EventLoop for serve protocol event dispatch

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Polish protocol.py — Error handling

**Files:**
- Modify: `cli/python/telos_tui/protocol.py`

**Interfaces:**
- Consumed by: `app.py` (already uses it)
- Produces: Same `ServeProtocol` API but with improved error tolerance

- [ ] **Step 1: Improve _read_events error handling**

In `protocol.py`, replace the `_read_events` method (lines 74-87) with:

```python
async def _read_events(self) -> None:
    """Read JSON lines from the subprocess stdout and enqueue them."""
    while self._running and self.process and self.process.stdout:
        try:
            line = await self.process.stdout.readline()
        except (BrokenPipeError, ConnectionResetError, OSError):
            await self._event_queue.put(
                {"type": "_error", "message": "Backend connection lost"}
            )
            break
        if not line:
            # EOF — process exited
            if self._running:
                await self._event_queue.put(
                    {"type": "_error", "message": "Backend process exited"}
                )
            break
        line_str = line.decode().strip()
        if not line_str:
            continue
        try:
            event = json.loads(line_str)
            await self._event_queue.put(event)
        except json.JSONDecodeError:
            # Skip malformed lines but don't crash
            pass
```

- [ ] **Step 2: Verify protocol module still imports**

Run: `cd cli/python && python -c "from telos_tui.protocol import ServeProtocol; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/protocol.py
git commit -m "fix: improve protocol error handling for connection loss

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 2: Refactor Existing Widgets

### Task 5: Update widgets/__init__.py — Exports

**Files:**
- Modify: `cli/python/telos_tui/widgets/__init__.py`

**Interfaces:**
- Produces: Named exports for all widget classes

- [ ] **Step 1: Write exports**

```python
"""telos TUI widgets."""

from .status_bar import StatusBar
from .approval_bar import ApprovalBar
from .input_panel import InputPanel
from .header import HeaderWidget
from .chat_view import ChatView
from .message_bubble import MessageBubble
from .tool_card import ToolCard

__all__ = [
    "StatusBar",
    "ApprovalBar",
    "InputPanel",
    "HeaderWidget",
    "ChatView",
    "MessageBubble",
    "ToolCard",
]
```

- [ ] **Step 2: Commit**

```bash
git add cli/python/telos_tui/widgets/__init__.py
git commit -m "feat: add widget exports

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: Refactor status_bar.py — Use state watching

**Files:**
- Modify: `cli/python/telos_tui/widgets/status_bar.py`

**Interfaces:**
- Consumes: `AppState` (through app.state reference)
- Produces: `StatusBar(Widget)` — watches `state.status_text`, `state.streaming`, `state.auto_approve`, etc.

- [ ] **Step 1: Rewrite status_bar.py to use AppState**

Replace the entire file:

```python
"""Status bar — watches AppState for status text, streaming, tokens."""

from datetime import datetime
from typing import TYPE_CHECKING, Optional

from rich.console import RenderableType
from rich.text import Text
from textual.widget import Widget

if TYPE_CHECKING:
    from ..state import AppState

BRAILLE_SPINNER: tuple[str, ...] = (
    "⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷",
)


class StatusBar(Widget):
    """Bottom status bar. Reads from AppState reactively via watch()."""

    DEFAULT_CSS = """
    StatusBar {
        height: 1;
        background: $panel;
        color: $text;
        padding: 0 1;
    }
    """

    def __init__(self) -> None:
        super().__init__()
        self._spinner_frame: int = 0
        self._started_at: Optional[datetime] = None
        self.set_interval(0.125, self._tick_spinner)

    @property
    def state(self) -> "AppState":
        return self.app.state  # type: ignore[attr-defined]

    def watch_state_status_text(self) -> None:
        """Re-render when status text changes."""
        self.refresh()

    def watch_state_streaming(self, streaming: bool) -> None:
        if streaming and self._started_at is None:
            self._started_at = datetime.now()
        elif not streaming:
            self._started_at = None
        self.refresh()

    def _tick_spinner(self) -> None:
        self._spinner_frame = (self._spinner_frame + 1) % len(BRAILLE_SPINNER)
        if self.state.streaming:
            self.refresh()

    def render(self) -> RenderableType:
        parts: list[Text] = []

        # Spinner during streaming
        if self.state.streaming:
            ch = BRAILLE_SPINNER[self._spinner_frame]
            parts.append(Text(ch, style="bold cyan"))
            parts.append(Text(" "))

        # Status text
        parts.append(Text(self.state.status_text, style="bold"))

        # Auto mode badge
        if self.state.auto_approve:
            parts.append(Text("  auto", style="bold yellow"))

        # Elapsed time during streaming
        if self.state.streaming and self._started_at:
            elapsed = (datetime.now() - self._started_at).total_seconds()
            if elapsed < 60:
                parts.append(Text(f"  {elapsed:.0f}s", style="dim"))
            else:
                m, s = divmod(int(elapsed), 60)
                parts.append(Text(f"  {m}m{s}s", style="dim"))

        # Tool count
        if self.state.tool_entries:
            ok = sum(1 for t in self.state.tool_entries if t.status == "ok")
            err = sum(1 for t in self.state.tool_entries if t.status == "error")
            run = sum(1 for t in self.state.tool_entries if t.status == "running")
            if err:
                parts.append(Text(f"  {ok}/{len(self.state.tool_entries)} tools · {err} failed", style="dim"))
            elif run:
                parts.append(Text(f"  {ok}/{len(self.state.tool_entries)} tools · {run} running", style="dim"))
            else:
                parts.append(Text(f"  {len(self.state.tool_entries)} tools", style="dim"))

        # Token budget bar
        total = self.state.input_tokens + self.state.output_tokens
        if self.state.token_budget_max > 0 and total > 0:
            pct = min(total / self.state.token_budget_max * 100, 100)
            bar_w = 10
            filled = max(int(round(pct / 100 * bar_w)), 1) if total > 0 else 0
            empty = bar_w - filled
            bar = "█" * filled + "░" * empty
            color = "bright_red" if pct >= 95 else "yellow" if pct >= 90 else "green"
            parts.append(Text(f"  {bar} {pct:.0f}%", style=color))

        return Text.assemble(*parts)
```

- [ ] **Step 2: Verify import**

Run: `cd cli/python && python -c "from telos_tui.widgets.status_bar import StatusBar; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/widgets/status_bar.py
git commit -m "refactor: StatusBar watches AppState instead of manual setters

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: Refactor approval_bar.py — Use state watching

**Files:**
- Modify: `cli/python/telos_tui/widgets/approval_bar.py`

**Interfaces:**
- Consumes: `AppState.pending_approval`
- Produces: `ApprovalBar(Widget)` — shows/hides based on `state.pending_approval`

- [ ] **Step 1: Rewrite approval_bar.py to use AppState**

```python
"""Inline approval bar — watches AppState.pending_approval."""

from typing import TYPE_CHECKING, Optional

from rich.console import RenderableType
from rich.panel import Panel
from rich.text import Text
from textual.widget import Widget

if TYPE_CHECKING:
    from ..state import AppState


class ApprovalBar(Widget):
    """Shows pending approval request. Visible only when state.pending_approval is set."""

    DEFAULT_CSS = """
    ApprovalBar {
        height: auto;
        background: $warning 20%;
        padding: 1 2;
        border: solid $warning;
        display: none;
    }
    ApprovalBar.-visible {
        display: block;
    }
    """

    @property
    def state(self) -> "AppState":
        return self.app.state  # type: ignore[attr-defined]

    def watch_state_pending_approval(
        self, request: Optional[dict]
    ) -> None:
        """Show/hide bar when approval state changes."""
        if request is not None:
            self.add_class("-visible")
        else:
            self.remove_class("-visible")
        self.refresh()

    def render(self) -> RenderableType:
        request = self.state.pending_approval
        if not request:
            return Text("")

        name = request.get("name", "?")
        reason = request.get("reason", "")
        args = request.get("arguments", {})

        # Compact argument display
        args_str = ""
        if isinstance(args, dict):
            for key in ("command", "file_path", "prompt", "url", "query", "pattern"):
                if key in args:
                    args_str = str(args[key])[:120]
                    break
            if not args_str:
                args_str = str(args)[:120]
        else:
            args_str = str(args)[:120]

        lines = [
            Text.assemble(
                ("! ", "bold red"),
                ("Approval required: ", "bold"),
                (name, "bold yellow"),
            ),
            Text(f"  {args_str}", style="dim"),
        ]
        if reason:
            lines.append(Text(f"  {reason}", style="dim"))
        lines.append(
            Text.assemble(
                ("  [", "dim"),
                ("y", "bold green"),
                ("] Allow  ", "dim"),
                ("[", "dim"),
                ("n", "bold red"),
                ("] Deny", "dim"),
            )
        )

        return Panel("\n".join(str(s) for s in lines), border_style="yellow")
```

- [ ] **Step 2: Verify import**

Run: `cd cli/python && python -c "from telos_tui.widgets.approval_bar import ApprovalBar; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/widgets/approval_bar.py
git commit -m "refactor: ApprovalBar watches AppState.pending_approval

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 8: Refactor input_panel.py — Clean up

**Files:**
- Modify: `cli/python/telos_tui/widgets/input_panel.py`

- [ ] **Step 1: Add a public Submitted message class with explicit text field**

Replace the file:

```python
"""Multi-line input panel with history navigation."""

from textual.binding import Binding
from textual.message import Message
from textual.widgets import TextArea


class InputPanel(TextArea):
    """Multi-line input area with prompt history (Ctrl+Up/Down)."""

    BINDINGS = [
        Binding("enter", "submit", "Send prompt", show=False),
        Binding("ctrl+up", "history_prev", "Previous prompt", show=False),
        Binding("ctrl+down", "history_next", "Next prompt", show=False),
    ]

    class Submitted(Message):
        """Posted when user presses Enter with non-empty input."""

        def __init__(self, sender: "InputPanel", text: str) -> None:
            super().__init__()
            self.text = text.strip()

    def __init__(self) -> None:
        super().__init__(
            text="",
            language=None,
            soft_wrap=True,
            show_line_numbers=False,
        )
        self._history: list[str] = []
        self._history_idx: int = -1
        self._draft: str = ""

    def clear_input(self) -> None:
        self.clear()
        self._history_idx = -1
        self._draft = ""

    def record_history(self, prompt: str) -> None:
        """Add a submitted prompt to history."""
        self._history.append(prompt)
        self._history_idx = -1
        self._draft = ""

    def action_submit(self) -> None:
        """Post Submit message when user presses Enter."""
        text = self.text.strip()
        if text:
            self.record_history(text)
            self.post_message(self.Submitted(self, text))
            self.clear()

    def action_history_prev(self) -> None:
        """Recall previous prompt from history."""
        if not self._history:
            return
        if self._history_idx == -1:
            self._draft = self.text
            self._history_idx = len(self._history) - 1
        elif self._history_idx > 0:
            self._history_idx -= 1
        self.text = self._history[self._history_idx]

    def action_history_next(self) -> None:
        """Move forward in history."""
        if self._history_idx == -1:
            return
        self._history_idx += 1
        if self._history_idx >= len(self._history):
            self._history_idx = -1
            self.text = self._draft
        else:
            self.text = self._history[self._history_idx]
```

- [ ] **Step 2: Verify import**

Run: `cd cli/python && python -c "from telos_tui.widgets.input_panel import InputPanel; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/widgets/input_panel.py
git commit -m "refactor: clean up InputPanel Submitted message

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 3: New Widgets

### Task 9: Create header.py — HeaderWidget

**Files:**
- Create: `cli/python/telos_tui/widgets/header.py`

**Interfaces:**
- Produces: `HeaderWidget(Widget)` — renders app title
- Consumed by: app.py compose()

- [ ] **Step 1: Create header.py**

```python
"""Header widget — shows application title."""

from rich.console import RenderableType
from rich.text import Text
from textual.widget import Widget


class HeaderWidget(Widget):
    """Top-of-screen header with app title."""

    DEFAULT_CSS = """
    HeaderWidget {
        height: 1;
        background: $panel;
        color: $text;
        padding: 0 1;
    }
    """

    def render(self) -> RenderableType:
        return Text("telos · AI Agent", style="bold cyan")
```

- [ ] **Step 2: Commit**

```bash
git add cli/python/telos_tui/widgets/header.py
git commit -m "feat: add HeaderWidget

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 10: Create message_bubble.py — Message rendering

**Files:**
- Create: `cli/python/telos_tui/widgets/message_bubble.py`

**Interfaces:**
- Consumes: `Message` dataclass from state.py
- Produces: `MessageBubble(Widget)` — renders one message with role-based styling
- Consumed by: ChatView

- [ ] **Step 1: Create message_bubble.py**

```python
"""Single message bubble with role-based styling."""

from rich.console import RenderableType
from rich.markdown import Markdown
from rich.panel import Panel
from rich.text import Text
from textual.widget import Widget

from ..state import Message


class MessageBubble(Widget):
    """Renders a single chat message with style matching its role."""

    DEFAULT_CSS = """
    MessageBubble {
        width: 1fr;
        height: auto;
        padding: 0 1;
    }
    """

    def __init__(self, message: Message) -> None:
        super().__init__()
        self.message = message

    def render(self) -> RenderableType:
        msg = self.message

        if msg.role == "user":
            return Panel(
                msg.text,
                title="You",
                border_style="cyan",
                title_align="left",
            )

        elif msg.role == "assistant":
            if not msg.text.strip():
                return Text("")
            try:
                md = Markdown(msg.text, code_theme="monokai")
                return Panel(
                    md,
                    title="Assistant",
                    border_style="green",
                    title_align="left",
                )
            except Exception:
                return Panel(msg.text, title="Assistant", border_style="green")

        elif msg.role == "thinking":
            return Text(f"  {msg.text}", style="dim italic")

        elif msg.role == "system":
            return Text(f"  {msg.text}", style="dim italic")

        elif msg.role == "tool":
            icon = {"running": "o", "ok": "v", "error": "x"}.get(
                msg.tool_status, "?"
            )
            style = {
                "running": "bold yellow",
                "ok": "bold green",
                "error": "bold red",
            }.get(msg.tool_status, "dim")
            detail = (
                msg.tool_detail[:80] + "..."
                if len(msg.tool_detail) > 80
                else msg.tool_detail
            )
            return Text.assemble(
                (f" {icon} ", style),
                (f"[{msg.tool_name}]", "bold"),
                (f"  {detail}", "dim"),
            )

        return Text(msg.text or "")
```

- [ ] **Step 2: Verify import**

Run: `cd cli/python && python -c "from telos_tui.widgets.message_bubble import MessageBubble; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/widgets/message_bubble.py
git commit -m "feat: add MessageBubble widget with per-role styling

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 11: Create chat_view.py — Scrollable message list

**Files:**
- Create: `cli/python/telos_tui/widgets/chat_view.py`

**Interfaces:**
- Consumes: `AppState.messages`
- Produces: `ChatView(VerticalScroll)` — list of MessageBubble widgets, auto-scrolls
- Consumed by: app.py compose()

- [ ] **Step 1: Create chat_view.py**

```python
"""Chat view — scrollable list of message bubbles."""

from typing import TYPE_CHECKING

from textual.containers import VerticalScroll
from textual.widget import Widget

from .message_bubble import MessageBubble

if TYPE_CHECKING:
    from ..state import AppState


class ChatView(VerticalScroll):
    """Scrollable container showing MessageBubble widgets for each message."""

    DEFAULT_CSS = """
    ChatView {
        height: 1fr;
        background: $surface;
        border: solid $primary;
    }
    """

    @property
    def state(self) -> "AppState":
        return self.app.state  # type: ignore[attr-defined]

    def watch_state_messages(self) -> None:
        """Rebuild children when messages list changes."""
        self._rebuild()

    def _rebuild(self) -> None:
        """Recreate all message bubbles. Textual handles diffing."""
        existing: list[Widget] = list(self.children)
        msgs = self.state.messages

        # If count doesn't match, full rebuild
        if len(existing) != len(msgs):
            self.remove_children()
            for msg in msgs:
                self.mount(MessageBubble(msg))
        else:
            # Update existing widgets in place
            for child, msg in zip(existing, msgs):
                if isinstance(child, MessageBubble):
                    child.message = msg
                    child.refresh()

        # Auto-scroll to bottom
        if msgs:
            self.scroll_end(animate=False)
```

- [ ] **Step 2: Verify import**

Run: `cd cli/python && python -c "from telos_tui.widgets.chat_view import ChatView; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/widgets/chat_view.py
git commit -m "feat: add ChatView with reactive message list rendering

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 12: Create tool_card.py — Expandable tool card

**Files:**
- Create: `cli/python/telos_tui/widgets/tool_card.py`

**Interfaces:**
- Consumes: `ToolEntry` from state.py
- Produces: `ToolCard(Widget)` — standalone collapsible tool entry card

- [ ] **Step 1: Create tool_card.py**

```python
"""Collapsible tool call card."""

from rich.console import RenderableType
from rich.panel import Panel
from rich.text import Text
from textual.widget import Widget

from ..state import ToolEntry


class ToolCard(Widget):
    """A single tool call card that can show/hide results."""

    DEFAULT_CSS = """
    ToolCard {
        width: 1fr;
        height: auto;
        padding: 0 2;
    }
    """

    def __init__(self, entry: ToolEntry) -> None:
        super().__init__()
        self.entry = entry
        self._expanded: bool = False

    def toggle(self) -> None:
        self._expanded = not self._expanded
        self.refresh()

    def render(self) -> RenderableType:
        e = self.entry
        icon = {"running": "o", "ok": "v", "error": "x"}.get(e.status, "?")
        style = {
            "running": "bold yellow",
            "ok": "bold green",
            "error": "bold red",
        }.get(e.status, "dim")

        header = Text.assemble(
            (f" {icon} ", style),
            (f"[{e.name}]", "bold"),
        )
        if e.detail:
            d = e.detail[:80] + "..." if len(e.detail) > 80 else e.detail
            header.append(Text(f"  {d}", style="dim"))

        if not self._expanded or not e.result_lines:
            return header

        result_lines = [str(header)]
        for line in e.result_lines[:6]:
            result_lines.append(f"    {line}")

        return Panel(
            "\n".join(result_lines),
            border_style="blue",
            padding=(0, 1),
        )
```

- [ ] **Step 2: Commit**

```bash
git add cli/python/telos_tui/widgets/tool_card.py
git commit -m "feat: add ToolCard collapsible widget

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 4: Assemble App

### Task 13: Rewrite app.py — Clean TUI application

**Files:**
- Modify: `cli/python/telos_tui/app.py`

**Interfaces:**
- Consumes: All modules from Tasks 1-12
- Produces: `TelosTuiApp(App)` — the runnable application

- [ ] **Step 1: Rewrite app.py**

```python
"""Textual TUI application for telos-agent chat."""

from __future__ import annotations

import asyncio

from textual import work
from textual.app import App, ComposeResult

from .event_loop import EventLoop
from .protocol import ServeProtocol
from .state import AppState, Message
from .widgets import (
    ApprovalBar,
    ChatView,
    HeaderWidget,
    InputPanel,
    StatusBar,
)


class TelosTuiApp(App):
    """Main Textual TUI — connects to `telos serve`, renders chat."""

    CSS = """
    HeaderWidget {
        dock: top;
    }

    ChatView {
        height: 1fr;
        border: solid $primary;
    }

    ApprovalBar {
        height: auto;
        background: $warning 20%;
        padding: 1 2;
        border: solid $warning;
        display: none;
    }
    ApprovalBar.-visible {
        display: block;
    }

    InputPanel {
        height: auto;
        max-height: 8;
        border: solid $primary-lighten-2;
        padding: 1;
    }

    StatusBar {
        dock: bottom;
    }
    """

    BINDINGS = [
        ("ctrl+c", "quit", "Quit"),
        ("escape", "focus_input", "Focus Input"),
        ("y", "approve_allow", "Allow"),
        ("n", "approve_deny", "Deny"),
    ]

    def __init__(self) -> None:
        super().__init__()
        self.state = AppState()
        self.event_loop = EventLoop(self.state)
        self.protocol = ServeProtocol()

    def compose(self) -> ComposeResult:
        yield HeaderWidget()
        yield ChatView()
        yield ApprovalBar()
        yield InputPanel()
        yield StatusBar()

    async def on_mount(self) -> None:
        self.input_widget = self.query_one(InputPanel)
        await self.protocol.start()
        self.state.connected = True
        self.state.status_text = "telos · connected"
        self.poll_events()

    async def on_unmount(self) -> None:
        await self.protocol.stop()

    @work(exclusive=False)
    async def poll_events(self) -> None:
        """Background worker: read events from serve process."""
        while True:
            event = await self.protocol.receive_event()
            if event is None:
                break
            self.call_from_thread(self.event_loop.handle_event, event)

    # ── input ──────────────────────────────────────────────────────

    def on_input_panel_submitted(self, event: InputPanel.Submitted) -> None:
        text = event.text
        if not text:
            return

        if text.startswith("/"):
            self._handle_slash(text)
            return

        # Show user message
        self.state.add_message(Message(role="user", text=text))
        self.state.streaming = True
        self.state.status_text = "telos · sending…"

        asyncio.create_task(self.protocol.send_command({"cmd": "run", "prompt": text}))

    def _handle_slash(self, text: str) -> None:
        parts = text.split(maxsplit=1)
        cmd = parts[0].lower()

        if cmd in ("/quit", "/exit"):
            asyncio.create_task(self.action_quit())
        elif cmd == "/clear":
            self.state.clear()
        elif cmd == "/new":
            asyncio.create_task(
                self.protocol.send_command({"cmd": "new_session"})
            )
        elif cmd == "/auto":
            self.state.auto_approve = not self.state.auto_approve
            s = "ON" if self.state.auto_approve else "OFF"
            self.state.add_message(Message(role="system", text=f"Auto-approve: {s}"))
        else:
            self.state.add_message(
                Message(role="system", text=f"Unknown command: {cmd}")
            )

    # ── approval ───────────────────────────────────────────────────

    def action_approve_allow(self) -> None:
        if self.state.pending_approval is not None:
            asyncio.create_task(
                self.protocol.send_command({"cmd": "_approve", "decision": "allow"})
            )
            self.state.pending_approval = None

    def action_approve_deny(self) -> None:
        if self.state.pending_approval is not None:
            asyncio.create_task(
                self.protocol.send_command({"cmd": "_approve", "decision": "deny"})
            )
            self.state.pending_approval = None

    def action_focus_input(self) -> None:
        try:
            self.input_widget.focus()
        except Exception:
            pass
```

- [ ] **Step 2: Verify app imports**

Run: `cd cli/python && python -c "from telos_tui.app import TelosTuiApp; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add cli/python/telos_tui/app.py
git commit -m "refactor: rewrite app.py with reactive state architecture

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 14: Cleanup old files

**Files:**
- Delete: `cli/python/telos_tui/widgets/tool_activity.py`
- Delete: `cli/python/telos_tui/widgets/chat_area.py`

- [ ] **Step 1: Delete deprecated files**

```bash
rm cli/python/telos_tui/widgets/tool_activity.py
rm cli/python/telos_tui/widgets/chat_area.py
```

- [ ] **Step 2: Verify everything still imports**

Run: `cd cli/python && python -c "from telos_tui.app import TelosTuiApp; print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git rm cli/python/telos_tui/widgets/tool_activity.py cli/python/telos_tui/widgets/chat_area.py
git commit -m "refactor: remove deprecated ToolActivityPanel and ChatArea

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 5: Polish & Final Assembly

### Task 15: Verify full import chain and pyproject.toml

**Files:**
- Verify: `cli/python/pyproject.toml`

- [ ] **Step 1: Verify pyproject.toml is valid**

Run: `cd cli/python && python -c "import tomllib; t=tomllib.load(open('pyproject.toml','rb')); print(t['project']['name'])"`
Expected: `telos-tui`

- [ ] **Step 2: Full import chain test**

Run:
```bash
cd cli/python && python -c "
from telos_tui.state import AppState, Message, ToolEntry
from telos_tui.streaming import StreamBuffer
from telos_tui.protocol import ServeProtocol
from telos_tui.event_loop import EventLoop
from telos_tui.widgets import (
    StatusBar, ApprovalBar, InputPanel,
    HeaderWidget, ChatView, MessageBubble, ToolCard,
)
from telos_tui.app import TelosTuiApp
print('All imports OK')
"
```
Expected: `All imports OK`

- [ ] **Step 3: State + streaming + event_loop unit tests**

Run:
```bash
cd cli/python && python -c "
from telos_tui.state import AppState, Message
s = AppState()
s.add_message(Message(role='user', text='hello'))
assert len(s.messages) == 1
assert s.messages[0].text == 'hello'
s.update_last_assistant('world')
s.clear()
assert len(s.messages) == 0
print('State OK')
"
```
Expected: `State OK`

Run:
```bash
cd cli/python && python -c "
from telos_tui.streaming import StreamBuffer
b = StreamBuffer(throttle_ms=0)
assert b.feed('Hello') == 'Hello'
assert b.feed(' world') == 'Hello world'
assert b.feed('\n\nnext') == 'Hello world\n\nnext'
b.reset()
b2 = StreamBuffer(throttle_ms=10000)
assert b2.feed('x') is None
print('Stream OK')
"
```
Expected: `Stream OK`

Run:
```bash
cd cli/python && python -c "
from telos_tui.state import AppState
from telos_tui.event_loop import EventLoop
s = AppState()
el = EventLoop(s)
el.handle_event({'type': 'TurnStarted'})
assert s.streaming is True
el.handle_event({'type': 'AssistantDelta', 'text': 'Hello'})
el.handle_event({'type': 'AssistantDelta', 'text': ' world'})
el.handle_event({'type': '_done'})
assert s.streaming is False
print('EventLoop OK')
"
```
Expected: `EventLoop OK`

- [ ] **Step 4: Commit**

```bash
git add cli/python/
git commit -m "chore: verify full import chain and pyproject.toml

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 16: Create README.md for the TUI

**Files:**
- Create: `cli/python/README.md`

- [ ] **Step 1: Create README.md**

```markdown
# telos-tui

Textual TUI frontend for [telos-agent](https://github.com/future-re/telos-agent).

## Architecture

```
telos-tui (Python Textual)  <->  telos serve (Rust daemon)
         JSON-line over stdin/stdout
```

- **`telos_tui/protocol.py`** — Subprocess management, JSON-line I/O
- **`telos_tui/state.py`** — Reactive AppState store (Textual reactive)
- **`telos_tui/event_loop.py`** — Event dispatch: protocol JSON -> state updates
- **`telos_tui/streaming.py`** — Streaming text buffer with throttle
- **`telos_tui/widgets/`** — Widget tree (ChatView, MessageBubble, StatusBar, etc.)

## Quick Start

```bash
pip install -e .
telos-tui
```

Requires `telos serve` on PATH (from telos-cli Rust binary).

## Key Bindings

| Key | Action |
|-----|--------|
| Enter | Send prompt |
| Ctrl+Up | Previous prompt |
| Ctrl+Down | Next prompt |
| Ctrl+C | Quit |
| y (during approval) | Allow tool call |
| n (during approval) | Deny tool call |
| Escape | Focus input |

## Slash Commands

| Command | Action |
|---------|--------|
| `/clear` | Clear chat |
| `/new` | New session |
| `/auto` | Toggle auto-approve |
| `/quit` | Exit |
```

- [ ] **Step 2: Commit**

```bash
git add cli/python/README.md
git commit -m "docs: add README for telos-tui Python package

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Verification Checklist

After all tasks are complete, run:

```bash
# 1. All imports
cd cli/python && python -c "from telos_tui.app import TelosTuiApp; print('OK')"

# 2. State tests
cd cli/python && python -c "
from telos_tui.state import AppState, Message
s = AppState()
s.add_message(Message(role='user', text='hello'))
assert len(s.messages) == 1
s.clear()
assert len(s.messages) == 0
print('State OK')
"

# 3. Streaming tests
cd cli/python && python -c "
from telos_tui.streaming import StreamBuffer
b = StreamBuffer(throttle_ms=0)
assert b.feed('Hello') == 'Hello'
b.reset()
b2 = StreamBuffer(throttle_ms=10000)
assert b2.feed('x') is None
print('Stream OK')
"

# 4. EventLoop tests
cd cli/python && python -c "
from telos_tui.state import AppState
from telos_tui.event_loop import EventLoop
s = AppState()
el = EventLoop(s)
el.handle_event({'type':'TurnStarted'})
assert s.streaming
el.handle_event({'type':'_done'})
assert not s.streaming
print('EventLoop OK')
"
```

Expected: all print `OK`
