from __future__ import annotations

import asyncio
from pathlib import Path
import sys
import unittest

from telos_tui.backend import BackendClient, build_serve_command


class BackendTests(unittest.IsolatedAsyncioTestCase):
    def test_build_serve_command_includes_shared_options(self) -> None:
        command = build_serve_command(provider="mock", model="demo", cwd=Path("/tmp/demo"))
        self.assertEqual(
            command,
            [
                "cargo",
                "run",
                "--quiet",
                "--bin",
                "telos",
                "--",
                "--provider",
                "mock",
                "--model",
                "demo",
                "--cwd",
                "/tmp/demo",
                "serve",
            ],
        )

    async def test_fake_backend_round_trip(self) -> None:
        fake_backend = Path(__file__).with_name("fake_backend.py")
        client = BackendClient(command=[sys.executable, str(fake_backend)])
        await client.start()

        await client.send_run("hi")
        first = await client.next_event()
        second = await client.next_event()

        self.assertEqual(first.kind, "AssistantDelta")
        self.assertEqual(first.payload["text"], "hello")
        self.assertEqual(second.kind, "_done")

        await client.shutdown()


if __name__ == "__main__":
    unittest.main()
