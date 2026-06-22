from __future__ import annotations

import unittest

from textual.widgets import Input, Static

from telos_tui.app import TelosTuiApp
from telos_tui.protocol import BackendEvent


class FakeBackend:
    def __init__(self) -> None:
        self.started = False
        self.prompts: list[str] = []
        self.events = []
        self.approvals: list[str] = []
        self.new_sessions = 0
        self.shutdown_called = False

    async def start(self) -> None:
        self.started = True

    async def send_run(self, prompt: str) -> None:
        self.prompts.append(prompt)
        self.events.extend(
            [
                BackendEvent(kind="AssistantDelta", payload={"type": "AssistantDelta", "text": "hello"}, raw={}),
                BackendEvent(kind="_done", payload={"type": "_done"}, raw={}),
            ]
        )

    async def send_new_session(self) -> None:
        self.new_sessions += 1
        self.events.append(BackendEvent(kind="_session_new", payload={"type": "_session_new"}, raw={}))

    async def send_approve(self, decision: str) -> None:
        self.approvals.append(decision)
        self.events.append(BackendEvent(kind="ApprovalResolved", payload={"decision": decision}, raw={}))

    async def shutdown(self) -> None:
        self.shutdown_called = True

    async def next_event(self) -> BackendEvent:
        while not self.events:
            import asyncio

            await asyncio.sleep(0.01)
        return self.events.pop(0)


class SmokeTests(unittest.IsolatedAsyncioTestCase):
    async def test_submit_prompt_updates_transcript(self) -> None:
        backend = FakeBackend()
        app = TelosTuiApp(backend=backend)

        async with app.run_test() as pilot:
            input_widget = app.query_one(Input)
            input_widget.value = "hi"
            await app.on_input_submitted(Input.Submitted(input_widget, "hi"))
            await pilot.pause()
            await pilot.pause()

            transcript = app.query_one("#transcript", Static)
            rendered = str(transcript.renderable)

            self.assertEqual(backend.prompts, ["hi"])
            self.assertIn("> hi", rendered)
            self.assertIn("hello", rendered)

    async def test_approval_event_blocks_input_and_shows_overlay(self) -> None:
        backend = FakeBackend()
        app = TelosTuiApp(backend=backend)

        async with app.run_test() as pilot:
            app._apply_backend_event(
                BackendEvent(
                    kind="_approval_required",
                    payload={"name": "Shell", "reason": "needs approval"},
                    raw={},
                )
            )
            await pilot.pause()

            input_widget = app.query_one(Input)
            overlay = app.query_one("#approval", Static)

            self.assertTrue(app.pending_approval)
            self.assertTrue(input_widget.disabled)
            self.assertIn("Approval required", str(overlay.renderable))

    async def test_session_new_resets_transcript_and_unblocks_input(self) -> None:
        backend = FakeBackend()
        app = TelosTuiApp(backend=backend)

        async with app.run_test() as pilot:
            app.transcript.append_user("before")
            app.pending_approval = True
            input_widget = app.query_one(Input)
            input_widget.disabled = True

            app._apply_backend_event(
                BackendEvent(kind="_session_new", payload={"type": "_session_new"}, raw={})
            )
            await pilot.pause()

            transcript = app.query_one("#transcript", Static)

            self.assertFalse(app.pending_approval)
            self.assertFalse(input_widget.disabled)
            self.assertEqual(app.transcript.cells, [])
            self.assertIn("Ready.", str(transcript.renderable))

    async def test_action_new_session_resets_and_sends_backend_command(self) -> None:
        backend = FakeBackend()
        app = TelosTuiApp(backend=backend)

        async with app.run_test() as pilot:
            app.transcript.append_user("before")
            app._render_transcript()

            await app.action_new_session()
            await pilot.pause()
            await pilot.pause()

            transcript = app.query_one("#transcript", Static)

            self.assertEqual(backend.new_sessions, 1)
            self.assertEqual(app.transcript.cells, [])
            self.assertIn("Ready.", str(transcript.renderable))

    async def test_approve_actions_send_decision(self) -> None:
        backend = FakeBackend()
        app = TelosTuiApp(backend=backend)

        async with app.run_test() as pilot:
            app._apply_backend_event(
                BackendEvent(
                    kind="_approval_required",
                    payload={"name": "Shell", "reason": "needs approval"},
                    raw={},
                )
            )

            await app.action_approve_allow()
            await pilot.pause()
            await pilot.pause()

            input_widget = app.query_one(Input)

            self.assertEqual(backend.approvals, ["allow"])
            self.assertFalse(app.pending_approval)
            self.assertFalse(input_widget.disabled)


if __name__ == "__main__":
    unittest.main()
