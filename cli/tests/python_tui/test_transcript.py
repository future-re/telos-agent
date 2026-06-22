from __future__ import annotations

import unittest

from telos_tui.transcript import TranscriptStore


class TranscriptTests(unittest.TestCase):
    def test_streaming_assistant_merges(self) -> None:
        store = TranscriptStore()
        store.append_assistant_delta("hel")
        store.append_assistant_delta("lo")
        self.assertEqual(len(store.cells), 1)
        self.assertEqual(store.cells[0].text, "hello")
        self.assertTrue(store.cells[0].streaming)

    def test_tool_lifecycle_updates_by_id(self) -> None:
        store = TranscriptStore()
        store.register_tool_call("tool-1", "Shell", "ls")
        store.append_tool_progress("tool-1", "running")
        store.complete_tool("tool-1", is_error=False, detail="done")
        store.apply_tool_result_message(
            {
                "blocks": [
                    {
                        "type": "ToolResult",
                        "data": {
                            "tool_call_id": "tool-1",
                            "name": "Shell",
                            "content": {"stdout": "file.txt"},
                            "is_error": False,
                        },
                    }
                ]
            }
        )
        cell = store.cells[0]
        self.assertEqual(cell.status, "completed")
        self.assertEqual(cell.detail, "done")
        self.assertIn("running", cell.lines)
        self.assertIn("stdout: file.txt", cell.lines)

    def test_done_and_reset(self) -> None:
        store = TranscriptStore()
        store.append_thinking_delta("hmm")
        store.mark_done()
        self.assertFalse(store.cells[0].streaming)
        store.reset()
        self.assertEqual(store.cells, [])


if __name__ == "__main__":
    unittest.main()
