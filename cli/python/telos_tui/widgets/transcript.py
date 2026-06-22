"""Transcript widgets for the Python TUI."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.containers import VerticalScroll
from textual.widgets import Static

from ..transcript import TranscriptCell
from .tool_call import summarize_tool


class TranscriptView(VerticalScroll):
    """Scrollable transcript surface."""

    def compose(self) -> ComposeResult:
        yield Static("Ready.", id="transcript")

    def render_cells(self, cells: list[TranscriptCell]) -> None:
        lines: list[str] = []
        for cell in cells:
            if cell.kind == "user":
                lines.append(f"> {cell.text}")
            elif cell.kind == "assistant":
                lines.append(cell.text)
            elif cell.kind == "thinking":
                lines.append(f"[thinking] {cell.text}")
            elif cell.kind == "tool":
                lines.append(summarize_tool(cell))
                lines.extend(f"  {line}" for line in cell.lines)
            elif cell.kind == "error":
                lines.append(f"[error] {cell.text}")
            elif cell.kind == "diagnostic":
                lines.append(f"[info] {cell.text}")
            elif cell.kind == "separator":
                lines.append("---")
        if not lines:
            lines.append("Ready.")

        self.query_one("#transcript", Static).update("\n".join(lines))
        self.scroll_end(animate=False)
