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

    def __init__(self) -> None:
        super().__init__()
        self._last_msg_count = 0
        self.set_interval(0.1, self._poll_messages)

    @property
    def state(self) -> "AppState":
        return self.app.state  # type: ignore[attr-defined]

    def _poll_messages(self) -> None:
        """Poll messages list and rebuild when it changes."""
        current = len(self.state.messages)
        if current != self._last_msg_count:
            self._last_msg_count = current
            self._rebuild()

    def _rebuild(self) -> None:
        """Recreate all message bubbles. Textual handles diffing."""
        existing: list[Widget] = list(self.children)
        msgs = self.state.messages
        count_changed = len(existing) != len(msgs)

        if count_changed:
            self.remove_children()
            for msg in msgs:
                self.mount(MessageBubble(msg))
        else:
            # Update existing widgets in place
            for child, msg in zip(existing, msgs):
                if isinstance(child, MessageBubble):
                    child.message = msg
                    child.refresh()

        # Auto-scroll to bottom only when a new message is added
        if count_changed and msgs:
            self.scroll_end(animate=False)
