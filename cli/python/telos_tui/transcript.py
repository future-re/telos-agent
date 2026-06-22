"""UI-agnostic transcript state for the Python TUI."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(slots=True)
class TranscriptCell:
    kind: str
    text: str = ""
    tool_call_id: str | None = None
    tool_name: str | None = None
    detail: str = ""
    status: str | None = None
    lines: list[str] = field(default_factory=list)
    data: dict[str, Any] = field(default_factory=dict)
    streaming: bool = False


class TranscriptStore:
    def __init__(self) -> None:
        self.cells: list[TranscriptCell] = []
        self._active_assistant: int | None = None
        self._active_thinking: int | None = None
        self._tool_index: dict[str, int] = {}

    def append_user(self, text: str) -> TranscriptCell:
        cell = TranscriptCell(kind="user", text=text)
        self.cells.append(cell)
        return cell

    def append_error(self, message: str) -> TranscriptCell:
        cell = TranscriptCell(kind="error", text=message)
        self.cells.append(cell)
        return cell

    def append_diagnostic(self, message: str, event: dict[str, Any] | None = None) -> TranscriptCell:
        cell = TranscriptCell(kind="diagnostic", text=message, data={"event": event or {}})
        self.cells.append(cell)
        return cell

    def append_separator(self) -> TranscriptCell:
        cell = TranscriptCell(kind="separator")
        self.cells.append(cell)
        return cell

    def append_assistant_delta(self, text: str) -> TranscriptCell:
        cell = self._get_or_create_streaming_cell("assistant")
        cell.text += text
        return cell

    def append_thinking_delta(self, text: str) -> TranscriptCell:
        cell = self._get_or_create_streaming_cell("thinking")
        cell.text += text
        return cell

    def register_tool_call(self, tool_call_id: str, name: str, detail: str) -> TranscriptCell:
        cell = TranscriptCell(
            kind="tool",
            tool_call_id=tool_call_id,
            tool_name=name,
            detail=detail,
            status="running",
        )
        self.cells.append(cell)
        self._tool_index[tool_call_id] = len(self.cells) - 1
        self.finish_streaming()
        return cell

    def append_tool_progress(self, tool_call_id: str, message: str) -> TranscriptCell | None:
        cell = self._find_tool(tool_call_id)
        if cell is None:
            return None
        cell.lines.append(message)
        return cell

    def complete_tool(self, tool_call_id: str, is_error: bool, detail: str | None = None) -> TranscriptCell | None:
        cell = self._find_tool(tool_call_id)
        if cell is None:
            return None
        cell.status = "error" if is_error else "completed"
        if detail:
            cell.detail = detail
        return cell

    def apply_tool_result_message(self, message: dict[str, Any]) -> None:
        for block in message.get("blocks", []):
            if not isinstance(block, dict):
                continue
            block_type = block.get("type")
            if block_type != "ToolResult":
                continue
            tool_result = block.get("data")
            if not isinstance(tool_result, dict):
                continue
            tool_call_id = tool_result.get("tool_call_id")
            if not isinstance(tool_call_id, str):
                continue
            cell = self._find_tool(tool_call_id)
            if cell is None:
                continue
            cell.status = "error" if tool_result.get("is_error") else "completed"
            cell.lines.extend(_preview_lines(tool_result.get("content")))

    def finish_streaming(self) -> None:
        if self._active_assistant is not None:
            self.cells[self._active_assistant].streaming = False
            self._active_assistant = None
        if self._active_thinking is not None:
            self.cells[self._active_thinking].streaming = False
            self._active_thinking = None

    def mark_done(self) -> None:
        self.finish_streaming()

    def reset(self) -> None:
        self.cells.clear()
        self._tool_index.clear()
        self._active_assistant = None
        self._active_thinking = None

    def _get_or_create_streaming_cell(self, kind: str) -> TranscriptCell:
        index_attr = "_active_assistant" if kind == "assistant" else "_active_thinking"
        active_index = getattr(self, index_attr)
        if active_index is not None:
            return self.cells[active_index]
        cell = TranscriptCell(kind=kind, streaming=True)
        self.cells.append(cell)
        setattr(self, index_attr, len(self.cells) - 1)
        return cell

    def _find_tool(self, tool_call_id: str) -> TranscriptCell | None:
        index = self._tool_index.get(tool_call_id)
        if index is None:
            return None
        return self.cells[index]


def _preview_lines(content: Any) -> list[str]:
    if isinstance(content, str):
        return content.splitlines()[:8]
    if isinstance(content, dict):
        items = [f"{key}: {value}" for key, value in content.items()]
        return items[:8]
    if isinstance(content, list):
        return [str(item) for item in content[:8]]
    return [str(content)]
