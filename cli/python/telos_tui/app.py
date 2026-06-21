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

        asyncio.create_task(
            self._safe_send_command({"cmd": "run", "prompt": text})
        )

    def _handle_slash(self, text: str) -> None:
        parts = text.split(maxsplit=1)
        cmd = parts[0].lower()

        if cmd in ("/quit", "/exit"):
            asyncio.create_task(self.action_quit())
        elif cmd == "/clear":
            self.state.clear()
        elif cmd == "/new":
            asyncio.create_task(
                self._safe_send_command({"cmd": "new_session"})
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
            asyncio.create_task(self._do_approve("allow"))

    def action_approve_deny(self) -> None:
        if self.state.pending_approval is not None:
            asyncio.create_task(self._do_approve("deny"))

    async def _do_approve(self, decision: str) -> None:
        """Send approval decision and clear pending state on success."""
        try:
            await self.protocol.send_command(
                {"cmd": "_approve", "decision": decision}
            )
            self.state.pending_approval = None
        except Exception:
            pass  # Keep approval visible so user can retry

    async def _safe_send_command(self, cmd: dict) -> None:
        """Send a command and log errors instead of silently swallowing them."""
        try:
            await self.protocol.send_command(cmd)
        except Exception as e:
            self.state.add_message(Message(role="system", text=f"Send error: {e}"))

    def action_focus_input(self) -> None:
        try:
            self.input_widget.focus()
        except Exception:
            pass
