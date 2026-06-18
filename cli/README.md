# telos-cli

Codex-style interactive terminal interface for [telos-agent](..).

## Features

- **Full-screen TUI:** Launch with `telos` for an immersive agent experience
- **Single-prompt mode:** `telos "refactor lib.rs"` for one-shot tasks
- **Context-aware:** Auto-discovers `CLAUDE.md`, `AGENTS.md`, git status
- **Streaming output:** Real-time markdown rendering with tool-call cards
- **Interactive approval:** Approve/deny tool calls inline
- **Session persistence:** Auto-saved to `.telos/sessions/`
- **Shell completions:** `telos completion bash|zsh`

## Build

From the workspace root:

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core
cargo build -p telos-cli
```

## Install

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core/cli
cargo install --path .
```

## Usage

### Full TUI (default)

```bash
telos --provider deepseek --api-key $DEEPSEEK_API_KEY
```

### Single prompt

```bash
telos --provider deepseek --api-key $DEEPSEEK_API_KEY "Refactor error handling"
```

### Environment variables

| Variable | Flag | Description |
|----------|------|-------------|
| `TELOS_PROVIDER` | `--provider` | Model provider (kimi, deepseek, mock) |
| `TELOS_MODEL` | `--model` | Model name |
| `TELOS_API_KEY` | `--api-key` | API key for the provider |
| `TELOS_CWD` | `--cwd` | Working directory |
| `TELOS_CONFIG` | `--config` | Path to config file |

### Config files

**User config** (`~/.config/telos/config.toml`):

```toml
[agent]
model = "deepseek-chat"
provider = "deepseek"
max_iterations = 16

[approval]
default_policy = "ask"

[approval.policies]
read = "allow"
shell = "ask"
write = "deny"
```

**Project config** (`.telos.toml` at project root):

```toml
[agent]
model = "deepseek-chat"
max_iterations = 32

[approval]
default_policy = "ask"
```

Project config overrides user config for matching keys. CLI flags override both.

### Approval policies

Each tool can have one of three policies:
- `allow` / `always-allow` — auto-approve without prompting
- `ask` / `always-ask` — prompt interactively (default)
- `deny` / `always-deny` — auto-deny without prompting

Configured per-tool in config files under `[approval.policies]`.

### Shell completions

```bash
telos completion bash > /usr/share/bash-completion/completions/telos
telos completion zsh  > /usr/local/share/zsh/site-functions/_telos
```

Run `telos --help` for all options.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Alt+Enter` | Insert newline in input |
| `Ctrl+D` | Quit when input is empty |
| `Ctrl+C` | Cancel current turn |
| `Ctrl+L` | Clear chat |
| `PgUp` / `PgDn` | Scroll chat |
| `a` / `y` | Approve pending tool call |
| `d` / `n` | Deny pending tool call |
| `e` | Request edit of pending tool call |

## License

`telos-cli` is licensed under the MIT License. It includes code adapted from
OpenAI's Codex CLI, which is licensed under the Apache License, Version 2.0.
See the `NOTICE` file for details.
