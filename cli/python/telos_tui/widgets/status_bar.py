"""Status bar widget."""

from __future__ import annotations

from textual.widgets import Static


class StatusBar(Static):
    """Compact status row."""

    def set_status(self, text: str) -> None:
        self.update(text)
