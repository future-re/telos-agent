"""Streaming text buffer with paragraph-boundary and throttle rendering."""

import time


class StreamBuffer:
    """Accumulates deltas and yields full text at sensible boundaries.

    Renders on paragraph breaks (double newline) or when enough time
    has passed since the last render.  Avoids re-rendering markdown on
    every single-token delta.
    """

    def __init__(self, throttle_ms: int = 50) -> None:
        self._buffer: list[str] = []
        self._throttle_ms = throttle_ms
        self._last_render = time.monotonic()
        self._rendered_len = 0

    def feed(self, delta: str) -> str | None:
        """Feed a delta. Returns the full text if it's time to render, else None."""
        self._buffer.append(delta)
        now = time.monotonic()
        elapsed_ms = (now - self._last_render) * 1000

        full = "".join(self._buffer)
        # Always render on a paragraph boundary
        if "\n\n" in full[self._rendered_len:]:
            self._rendered_len = len(full)
            self._last_render = now
            return full
        # Throttle to at most every throttle_ms
        if elapsed_ms >= self._throttle_ms and len(full) > self._rendered_len:
            self._rendered_len = len(full)
            self._last_render = now
            return full
        return None

    def flush(self) -> str | None:
        """Force-render whatever remains. Returns None if nothing new."""
        full = "".join(self._buffer)
        if len(full) > self._rendered_len:
            self._rendered_len = len(full)
            return full
        return None

    def reset(self) -> None:
        """Reset for a new streaming message."""
        self._buffer = []
        self._rendered_len = 0
        self._last_render = time.monotonic()
