from __future__ import annotations

import unittest

from telos_tui.protocol import approve_command, new_session_command, parse_event_line, quit_command, run_command


class ProtocolTests(unittest.TestCase):
    def test_command_helpers(self) -> None:
        self.assertEqual(run_command("hi"), {"cmd": "run", "prompt": "hi"})
        self.assertEqual(new_session_command(), {"cmd": "new_session"})
        self.assertEqual(approve_command("allow"), {"cmd": "_approve", "decision": "allow"})
        self.assertEqual(quit_command(), {"cmd": "quit"})

    def test_parse_known_event(self) -> None:
        event = parse_event_line('{"type":"AssistantDelta","text":"hi"}')
        self.assertEqual(event.kind, "AssistantDelta")
        self.assertEqual(event.payload["text"], "hi")

    def test_parse_unknown_event(self) -> None:
        event = parse_event_line('{"type":"Mystery","x":1}')
        self.assertEqual(event.kind, "diagnostic")
        self.assertIn("unknown event type", event.payload["message"])

    def test_parse_invalid_json(self) -> None:
        event = parse_event_line("{bad")
        self.assertEqual(event.kind, "diagnostic")
        self.assertIn("invalid json", event.payload["message"])


if __name__ == "__main__":
    unittest.main()
