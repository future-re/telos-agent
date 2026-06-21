#!/usr/bin/env python3
"""Entry point for the telos Textual TUI."""
import asyncio
import sys

from .app import TelosTuiApp


def main():
    try:
        asyncio.run(TelosTuiApp().run_async())
    except KeyboardInterrupt:
        pass
    except Exception as e:
        print(f"Fatal error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
