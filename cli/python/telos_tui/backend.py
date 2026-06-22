"""Async subprocess client for `telos serve`."""

from __future__ import annotations

import asyncio
import contextlib
import json
from pathlib import Path
from typing import Any

from .protocol import (
    BackendEvent,
    approve_command,
    new_session_command,
    parse_event_line,
    quit_command,
    run_command,
)


class BackendClient:
    def __init__(
        self,
        command: list[str] | None = None,
        *,
        cwd: str | Path | None = None,
        provider: str | None = None,
        model: str | None = None,
    ) -> None:
        self.command = command or build_serve_command(provider=provider, model=model, cwd=cwd)
        self.cwd = str(cwd) if cwd is not None else None
        self.process: asyncio.subprocess.Process | None = None
        self.events: asyncio.Queue[BackendEvent] = asyncio.Queue()
        self._stdout_task: asyncio.Task[None] | None = None
        self._stderr_task: asyncio.Task[None] | None = None

    async def start(self) -> None:
        if self.process is not None:
            return
        self.process = await asyncio.create_subprocess_exec(
            *self.command,
            cwd=self.cwd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        self._stdout_task = asyncio.create_task(self._read_stdout())
        self._stderr_task = asyncio.create_task(self._read_stderr())

    async def send_run(self, prompt: str) -> None:
        await self._send(run_command(prompt))

    async def send_new_session(self) -> None:
        await self._send(new_session_command())

    async def send_approve(self, decision: str) -> None:
        await self._send(approve_command(decision))

    async def shutdown(self) -> None:
        if self.process is None:
            return
        with contextlib.suppress(RuntimeError, BrokenPipeError):
            await self._send(quit_command())
        if self.process.returncode is None:
            self.process.terminate()
            await self.process.wait()
        for task in (self._stdout_task, self._stderr_task):
            if task is not None:
                task.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await task
        self.process = None

    async def next_event(self) -> BackendEvent:
        return await self.events.get()

    async def _send(self, payload: dict[str, Any]) -> None:
        if self.process is None or self.process.stdin is None:
            raise RuntimeError("backend process not started")
        line = json.dumps(payload) + "\n"
        self.process.stdin.write(line.encode("utf-8"))
        await self.process.stdin.drain()

    async def _read_stdout(self) -> None:
        assert self.process is not None
        assert self.process.stdout is not None
        while True:
            line = await self.process.stdout.readline()
            if not line:
                await self.events.put(
                    BackendEvent(
                        kind="backend_exit",
                        payload={"returncode": self.process.returncode},
                        raw={"returncode": self.process.returncode},
                    )
                )
                return
            await self.events.put(parse_event_line(line.decode("utf-8").rstrip("\n")))

    async def _read_stderr(self) -> None:
        assert self.process is not None
        assert self.process.stderr is not None
        while True:
            line = await self.process.stderr.readline()
            if not line:
                return
            text = line.decode("utf-8", errors="replace").rstrip("\n")
            await self.events.put(BackendEvent(kind="stderr", payload={"text": text}, raw={"text": text}))


def build_serve_command(
    *,
    provider: str | None = None,
    model: str | None = None,
    cwd: str | Path | None = None,
) -> list[str]:
    command = ["cargo", "run", "--quiet", "--bin", "telos", "--"]
    if provider:
        command.extend(["--provider", provider])
    if model:
        command.extend(["--model", model])
    if cwd is not None:
        command.extend(["--cwd", str(cwd)])
    command.append("serve")
    return command
