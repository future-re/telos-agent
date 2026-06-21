# telos-tui

Textual TUI frontend for [telos-agent](https://github.com/future-re/telos-agent).

## Architecture

```
telos-tui (Python Textual)  <->  telos serve (Rust daemon)
         JSON-line over stdin/stdout
```

- **`telos_tui/protocol.py`** — Subprocess management, JSON-line I/O
- **`telos_tui/state.py`** — Reactive AppState store (Textual reactive)
- **`telos_tui/event_loop.py`** — Event dispatch: protocol JSON -> state updates
- **`telos_tui/streaming.py`** — Streaming text buffer with throttle
- **`telos_tui/widgets/`** — Widget tree (ChatView, MessageBubble, StatusBar, etc.)

## Quick Start

```bash
pip install -e .
telos-tui
```

Requires `telos serve` on PATH (from telos-cli Rust binary).

## Key Bindings

| Key | Action |
|-----|--------|
| Enter | Send prompt |
| Ctrl+Up | Previous prompt |
| Ctrl+Down | Next prompt |
| Ctrl+C | Quit |
| y (during approval) | Allow tool call |
| n (during approval) | Deny tool call |
| Escape | Focus input |

## Slash Commands

| Command | Action |
|---------|--------|
| `/clear` | Clear chat |
| `/new` | New session |
| `/auto` | Toggle auto-approve |
| `/quit` | Exit |
