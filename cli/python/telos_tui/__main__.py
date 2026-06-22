"""`python -m telos_tui` entrypoint."""

from __future__ import annotations

import asyncio
import sys

from .app import run


def main() -> int:
    return asyncio.run(run(sys.argv[1:]))


if __name__ == "__main__":
    raise SystemExit(main())
