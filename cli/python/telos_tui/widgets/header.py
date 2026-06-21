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
