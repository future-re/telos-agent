"""Multi-line input panel with history navigation."""

from textual.binding import Binding
from textual.message import Message
from textual.widgets import TextArea


class InputPanel(TextArea):
    """Multi-line input area with prompt history (Ctrl+Up/Down)."""

    BINDINGS = [
        Binding("enter", "submit", "Send prompt", show=False),
        Binding("ctrl+up", "history_prev", "Previous prompt", show=False),
        Binding("ctrl+down", "history_next", "Next prompt", show=False),
    ]

    class Submitted(Message):
        """Posted when user presses Enter with non-empty input."""

        def __init__(self, sender: "InputPanel", text: str) -> None:
            super().__init__()
            self.text = text.strip()

    def __init__(self) -> None:
        super().__init__(
            text="",
            language=None,
            soft_wrap=True,
            show_line_numbers=False,
        )
        self._history: list[str] = []
        self._history_idx: int = -1
        self._draft: str = ""

    def clear_input(self) -> None:
        self.clear()
        self._history_idx = -1
        self._draft = ""

    def record_history(self, prompt: str) -> None:
        """Add a submitted prompt to history."""
        self._history.append(prompt)
        self._history_idx = -1
        self._draft = ""

    def action_submit(self) -> None:
        """Post Submit message when user presses Enter."""
        text = self.text.strip()
        if text:
            self.record_history(text)
            self.post_message(self.Submitted(self, text))
            self.clear()

    def action_history_prev(self) -> None:
        """Recall previous prompt from history."""
        if not self._history:
            return
        if self._history_idx == -1:
            self._draft = self.text
            self._history_idx = len(self._history) - 1
        elif self._history_idx > 0:
            self._history_idx -= 1
        self.text = self._history[self._history_idx]

    def action_history_next(self) -> None:
        """Move forward in history."""
        if self._history_idx == -1:
            return
        self._history_idx += 1
        if self._history_idx >= len(self._history):
            self._history_idx = -1
            self.text = self._draft
        else:
            self.text = self._history[self._history_idx]
