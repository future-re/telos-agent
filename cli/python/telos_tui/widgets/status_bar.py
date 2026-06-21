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
