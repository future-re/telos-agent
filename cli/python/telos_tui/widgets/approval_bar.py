"""Inline approval bar — watches AppState.pending_approval via polling."""

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

    def __init__(self) -> None:
        super().__init__()
        self.set_interval(0.25, self._poll_approval)

    @property
    def state(self) -> "AppState":
        return self.app.state  # type: ignore[attr-defined]

    def _poll_approval(self) -> None:
        """Poll pending_approval and toggle visibility."""
        request = self.state.pending_approval
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

        result = Text()
        for i, s in enumerate(lines):
            if i > 0:
                result.append("\n")
            if isinstance(s, Text):
                result.append_text(s)
            else:
                result.append(str(s))
        return Panel(result, border_style="yellow")
