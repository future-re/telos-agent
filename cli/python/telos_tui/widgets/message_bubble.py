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
