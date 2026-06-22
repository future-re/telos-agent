"""Tool call rendering helpers."""

from __future__ import annotations

from ..transcript import TranscriptCell


def summarize_tool(cell: TranscriptCell) -> str:
    status = cell.status or "running"
    name = cell.tool_name or "tool"
    marker = {
        "running": "...",
        "completed": "ok",
        "error": "!!",
    }.get(status, status)
    return f"[{marker}] {name} {cell.detail}".rstrip()
