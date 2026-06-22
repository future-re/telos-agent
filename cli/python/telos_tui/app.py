"""Minimal Textual app shell for the Python TUI."""

from __future__ import annotations

import argparse
import asyncio
from pathlib import Path
from typing import Any

from textual.app import App, ComposeResult
from textual.containers import Container
from textual.widgets import Footer, Header, Input

from .backend import BackendClient
from .protocol import BackendEvent
from .transcript import TranscriptStore
from .widgets.approval import ApprovalOverlay
from .widgets.prompt import PromptInput
from .widgets.status_bar import StatusBar
from .widgets.transcript import TranscriptView


class TelosTuiApp(App[None]):
    CSS_PATH = "styles.tcss"

    BINDINGS = [
        ("ctrl+d", "quit", "Quit"),
        ("ctrl+l", "new_session", "New Session"),
        ("y", "approve_allow", "Approve"),
        ("n", "approve_deny", "Deny"),
        ("escape", "approve_deny", "Deny"),
    ]

    def __init__(self, backend: BackendClient | None = None) -> None:
        super().__init__()
        self.backend = backend or BackendClient()
        self.transcript = TranscriptStore()
        self.pending_approval = False
        self._event_task: asyncio.Task[None] | None = None

    def compose(self) -> ComposeResult:
        yield Header()
        with Container(id="body"):
            yield TranscriptView(id="transcript-view")
            yield ApprovalOverlay(id="approval")
            yield PromptInput(placeholder="Send a prompt", id="prompt")
            yield StatusBar("Ready", id="status-bar")
        yield Footer()

    async def on_mount(self) -> None:
        try:
            await self.backend.start()
        except Exception as exc:
            self.transcript.append_error(f"backend start failed: {exc}")
            self._render_transcript()
            self.query_one(PromptInput).set_blocked(True)
            self.query_one(StatusBar).set_status("Backend start failed")
            return
        self._event_task = asyncio.create_task(self._consume_backend_events())
        self.query_one(StatusBar).set_status("Connected")
        self._render_transcript()

    async def on_unmount(self) -> None:
        if self._event_task is not None:
            self._event_task.cancel()
            try:
                await self._event_task
            except asyncio.CancelledError:
                pass
        await self.backend.shutdown()

    async def on_input_submitted(self, event: Input.Submitted) -> None:
        prompt = event.value.strip()
        if not prompt or self.pending_approval:
            return
        self.transcript.append_user(prompt)
        self.transcript.finish_streaming()
        self.query_one(StatusBar).set_status("Running turn")
        self._render_transcript()
        event.input.value = ""
        try:
            await self.backend.send_run(prompt)
        except Exception as exc:
            self.transcript.append_error(f"failed to submit prompt: {exc}")
            self.query_one(StatusBar).set_status("Submit failed")
            self._render_transcript()

    async def action_new_session(self) -> None:
        self.transcript.reset()
        self.pending_approval = False
        self.query_one(PromptInput).set_blocked(False)
        self.query_one(ApprovalOverlay).hide_request()
        self.query_one(StatusBar).set_status("New session")
        self._render_transcript()
        try:
            await self.backend.send_new_session()
        except Exception as exc:
            self.transcript.append_error(f"failed to request new session: {exc}")
            self.query_one(StatusBar).set_status("New session failed")
            self._render_transcript()

    async def action_approve_allow(self) -> None:
        await self._resolve_approval("allow")

    async def action_approve_deny(self) -> None:
        await self._resolve_approval("deny")

    async def _consume_backend_events(self) -> None:
        while True:
            event = await self.backend.next_event()
            self._apply_backend_event(event)
            self._render_transcript()

    def _apply_backend_event(self, event: BackendEvent) -> None:
        payload = event.payload
        kind = event.kind

        if kind == "AssistantDelta":
            text = payload.get("text")
            if isinstance(text, str):
                self.transcript.append_assistant_delta(text)
            return

        if kind == "ThinkingDelta":
            text = payload.get("text")
            if isinstance(text, str):
                self.transcript.append_thinking_delta(text)
            return

        if kind == "ToolCall":
            tool_call_id = payload.get("tool_call_id")
            name = payload.get("name")
            detail = payload.get("detail", "")
            if isinstance(tool_call_id, str) and isinstance(name, str) and isinstance(detail, str):
                self.transcript.register_tool_call(tool_call_id, name, detail)
                self.query_one(StatusBar).set_status(f"Tool: {name}")
            return

        if kind == "ToolProgress":
            tool_call_id = payload.get("tool_call_id")
            message = payload.get("message")
            if isinstance(tool_call_id, str) and isinstance(message, str):
                self.transcript.append_tool_progress(tool_call_id, message)
            return

        if kind == "ToolCompleted":
            tool_call_id = payload.get("tool_call_id")
            is_error = payload.get("is_error")
            detail = payload.get("detail")
            if isinstance(tool_call_id, str) and isinstance(is_error, bool):
                self.transcript.complete_tool(
                    tool_call_id,
                    is_error=is_error,
                    detail=detail if isinstance(detail, str) else None,
                )
            return

        if kind == "ToolResult":
            self.transcript.apply_tool_result_message(_normalize_message_payload(payload))
            return

        if kind == "_approval_required":
            self.pending_approval = True
            name = payload.get("name", "tool")
            reason = payload.get("reason", "")
            self.transcript.append_diagnostic(f"approval required: {name} {reason}".strip(), payload)
            self.query_one(PromptInput).set_blocked(True)
            self.query_one(ApprovalOverlay).show_request(str(name), str(reason))
            self.query_one(StatusBar).set_status("Approval required")
            return

        if kind == "ApprovalResolved":
            self.pending_approval = False
            self.query_one(PromptInput).set_blocked(False)
            self.query_one(ApprovalOverlay).hide_request()
            decision = payload.get("decision", "")
            self.transcript.append_diagnostic(f"approval resolved: {decision}".strip(), payload)
            self.query_one(StatusBar).set_status(f"Approval: {decision}")
            return

        if kind == "_session_new":
            self.pending_approval = False
            self.transcript.reset()
            self.query_one(PromptInput).set_blocked(False)
            self.query_one(ApprovalOverlay).hide_request()
            self.query_one(StatusBar).set_status("New session")
            return

        if kind == "_done":
            self.transcript.mark_done()
            self.query_one(StatusBar).set_status("Turn complete")
            return

        if kind == "_error":
            message = payload.get("message", "backend error")
            self.transcript.append_error(str(message))
            self.query_one(StatusBar).set_status("Backend error")
            return

        if kind == "stderr":
            text = payload.get("text")
            if isinstance(text, str) and text:
                self.transcript.append_diagnostic(text, payload)
            return

        if kind == "backend_exit":
            self.transcript.append_error("backend exited")
            self.query_one(PromptInput).set_blocked(True)
            self.query_one(StatusBar).set_status("Backend exited")
            return

        if kind == "diagnostic":
            self.transcript.append_diagnostic(str(payload.get("message", "diagnostic")), payload)
            return

    def _render_transcript(self) -> None:
        self.query_one(TranscriptView).render_cells(self.transcript.cells)

    async def _resolve_approval(self, decision: str) -> None:
        if not self.pending_approval:
            return
        try:
            await self.backend.send_approve(decision)
        except Exception as exc:
            self.transcript.append_error(f"failed to send approval: {exc}")
            self.query_one(StatusBar).set_status("Approval send failed")
            self._render_transcript()


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m telos_tui")
    parser.add_argument("--provider")
    parser.add_argument("--model")
    parser.add_argument("--cwd", type=Path)
    parser.add_argument("--backend-cmd", nargs="+")
    return parser


async def run(argv: list[str] | None = None) -> int:
    parser = build_arg_parser()
    args = parser.parse_args(argv)
    backend = BackendClient(
        command=args.backend_cmd,
        cwd=args.cwd,
        provider=args.provider,
        model=args.model,
    )
    app = TelosTuiApp(backend=backend)
    await app.run_async()
    return 0


def _normalize_message_payload(payload: dict[str, Any]) -> dict[str, Any]:
    if "blocks" in payload and isinstance(payload["blocks"], list):
        return payload
    blocks = payload.get("blocks")
    if blocks is None:
        return {"blocks": []}
    return {"blocks": blocks if isinstance(blocks, list) else []}
