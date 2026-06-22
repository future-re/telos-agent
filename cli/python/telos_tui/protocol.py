"""Protocol helpers for the `telos serve` JSONL bridge."""

from __future__ import annotations

from dataclasses import dataclass
import json
from typing import Any


def run_command(prompt: str) -> dict[str, Any]:
    return {"cmd": "run", "prompt": prompt}


def new_session_command() -> dict[str, str]:
    return {"cmd": "new_session"}


def approve_command(decision: str) -> dict[str, str]:
    return {"cmd": "_approve", "decision": decision}


def quit_command() -> dict[str, str]:
    return {"cmd": "quit"}


@dataclass(slots=True)
class BackendEvent:
    kind: str
    payload: dict[str, Any]
    raw: dict[str, Any]


def parse_event_line(line: str) -> BackendEvent:
    try:
        raw = json.loads(line)
    except json.JSONDecodeError as exc:
        return BackendEvent(
            kind="diagnostic",
            payload={"message": f"invalid json: {exc}", "line": line[:400]},
            raw={"line": line},
        )

    if not isinstance(raw, dict):
        return BackendEvent(
            kind="diagnostic",
            payload={"message": "event must be a JSON object", "line": line[:400]},
            raw={"value": raw},
        )

    event_type = raw.get("type")
    if not isinstance(event_type, str) or not event_type:
        return BackendEvent(
            kind="diagnostic",
            payload={"message": "event missing string type", "event": raw},
            raw=raw,
        )

    if event_type in {
        "AssistantDelta",
        "ThinkingDelta",
        "ToolCall",
        "ToolProgress",
        "ToolCompleted",
        "ToolResult",
        "ProviderUsage",
        "TurnStarted",
        "IterationStarted",
        "ProviderRequest",
        "CompactionStarted",
        "CompactionCompleted",
        "TokenBudgetExceeded",
        "HookStarted",
        "HookCompleted",
        "ApprovalRequested",
        "ApprovalResolved",
        "ProviderRetry",
        "TurnFinished",
        "User",
        "Assistant",
        "_approval_required",
        "_done",
        "_error",
        "_session_new",
    }:
        return BackendEvent(kind=event_type, payload=raw, raw=raw)

    return BackendEvent(
        kind="diagnostic",
        payload={"message": f"unknown event type: {event_type}", "event": raw},
        raw=raw,
    )
