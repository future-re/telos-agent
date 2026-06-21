"""JSON-line protocol handler for communicating with `telos serve`."""

import asyncio
import json
import os
import sys
from typing import Optional


class ServeProtocol:
    """Manages the subprocess and JSON-line I/O with `telos serve`."""

    def __init__(self, bin_path: str = "telos", args: Optional[list[str]] = None):
        self.bin_path = bin_path
        self.args = args or []
        self.process: Optional[asyncio.subprocess.Process] = None
        self._reader_task: Optional[asyncio.Task] = None
        self._event_queue: asyncio.Queue = asyncio.Queue()
        self._running = False

    async def start(self) -> None:
        """Launch the `telos serve` subprocess."""
        cmd = [self.bin_path, "serve"] + self.args
        env = os.environ.copy()
        # Ensure unbuffered stdout
        env.setdefault("PYTHONUNBUFFERED", "1")

        self.process = await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=sys.stderr,
            env=env,
        )
        self._running = True
        self._reader_task = asyncio.create_task(self._read_events())

    async def stop(self) -> None:
        """Gracefully stop the subprocess."""
        self._running = False
        if self._reader_task:
            self._reader_task.cancel()
            try:
                await self._reader_task
            except asyncio.CancelledError:
                pass
        if self.process and self.process.returncode is None:
            try:
                self.process.stdin.write(b'{"cmd":"quit"}\n')
                await self.process.stdin.drain()
            except Exception:
                pass
            try:
                self.process.terminate()
                await asyncio.wait_for(self.process.wait(), timeout=3)
            except Exception:
                self.process.kill()

    async def send_command(self, cmd: dict) -> None:
        """Send a JSON command to the serve process."""
        if not self.process or not self.process.stdin:
            return
        line = json.dumps(cmd) + "\n"
        self.process.stdin.write(line.encode())
        await self.process.stdin.drain()

    async def receive_event(self) -> Optional[dict]:
        """Get the next event from the queue. Returns None if stopped."""
        try:
            return await self._event_queue.get()
        except asyncio.CancelledError:
            return None

    async def _read_events(self) -> None:
        """Read JSON lines from the subprocess stdout and enqueue them."""
        while self._running and self.process and self.process.stdout:
            try:
                line = await self.process.stdout.readline()
            except (BrokenPipeError, ConnectionResetError, OSError):
                await self._event_queue.put(
                    {"type": "_error", "message": "Backend connection lost"}
                )
                break
            if not line:
                # EOF — process exited
                if self._running:
                    await self._event_queue.put(
                        {"type": "_error", "message": "Backend process exited"}
                    )
                break
            line_str = line.decode().strip()
            if not line_str:
                continue
            try:
                event = json.loads(line_str)
                await self._event_queue.put(event)
            except json.JSONDecodeError:
                # Skip malformed lines but don't crash
                pass
