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

        result = Text()
        result.append_text(header)
        for line in e.result_lines[:6]:
            result.append("\n    ")
            if isinstance(line, Text):
                result.append_text(line)
            else:
                result.append(str(line))

        return Panel(
            result,
            border_style="blue",
            padding=(0, 1),
        )
