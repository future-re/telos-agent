"""Approval overlay widget."""

from __future__ import annotations

from textual.widgets import Static


class ApprovalOverlay(Static):
    """Simple in-screen approval panel for the first milestone."""

    def show_request(self, name: str, reason: str) -> None:
        self.update(f"Approval required\n{name}\n{reason}".strip())
        self.display = True

    def hide_request(self) -> None:
        self.update("")
        self.display = False
