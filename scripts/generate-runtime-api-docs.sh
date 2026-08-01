#!/usr/bin/env bash
set -euo pipefail

cargo doc -p telos_agent --no-deps

echo
echo "Generated runtime API docs:"
echo "  target/doc/telos_agent/index.html"
