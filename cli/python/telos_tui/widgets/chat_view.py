"""Chat view — scrollable list of message bubbles."""

from typing import TYPE_CHECKING

from textual.containers import VerticalScroll
from textual.widget import Widget

from .message_bubble import MessageBubble

if TYPE_CHECKING:
    from ..state import AppState


class ChatView(VerticalScroll):
    """Scrollable container showing MessageBubble widgets for each message."""

    DEFAULT_CSS = """
    ChatView {
        height: 1fr;
        background: $surface;
        border: solid $primary;
    }
    """

    @property
    def state(self) -> "AppState":
        return self.app.state  # type: ignore[attr-defined]

    def watch_state_messages(self) -> None:
        """Rebuild children when messages list changes."""
        self._rebuild()

    def _rebuild(self) -> None:
        """Recreate all message bubbles. Textual handles diffing."""
        existing: list[Widget] = list(self.children)
        msgs = self.state.messages

        # If count doesn't match, full rebuild
        if len(existing) != len(msgs):
            self.remove_children()
            for msg in msgs:
                self.mount(MessageBubble(msg))
        else:
            # Update existing widgets in place
            for child, msg in zip(existing, msgs):
                if isinstance(child, MessageBubble):
                    child.message = msg
                    child.refresh()

        # Auto-scroll to bottom
        if msgs:
            self.scroll_end(animate=False)
