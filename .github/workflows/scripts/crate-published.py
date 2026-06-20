#!/usr/bin/env python3
"""Exit 0 when a crate version is published on crates.io, else exit 1."""

from __future__ import annotations

import json
import sys
import urllib.error
import urllib.request


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: crate-published.py <crate> <version>", file=sys.stderr)
        return 2

    crate_name = sys.argv[1]
    version = sys.argv[2]
    url = f"https://crates.io/api/v1/crates/{crate_name}/{version}"
    request = urllib.request.Request(url, headers={"User-Agent": "telos-agent-release"})

    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return 1
        raise

    published = payload.get("version", {}).get("num") == version
    return 0 if published else 1


if __name__ == "__main__":
    raise SystemExit(main())
