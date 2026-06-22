from __future__ import annotations

import json
import sys


def main() -> int:
    for line in sys.stdin:
        command = json.loads(line)
        cmd = command["cmd"]
        if cmd == "run":
            print(json.dumps({"type": "AssistantDelta", "text": "hello"}), flush=True)
            print(json.dumps({"type": "_done"}), flush=True)
        elif cmd == "new_session":
            print(json.dumps({"type": "_session_new"}), flush=True)
        elif cmd == "_approve":
            print(json.dumps({"type": "ApprovalResolved", "decision": command["decision"]}), flush=True)
        elif cmd == "quit":
            return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
