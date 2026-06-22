"""Prompt input widget."""

from __future__ import annotations

from textual.widgets import Input


class PromptInput(Input):
    """Bottom composer for prompt submission."""

    def set_blocked(self, blocked: bool) -> None:
        self.disabled = blocked
        self.placeholder = "Approval pending" if blocked else "Send a prompt"
