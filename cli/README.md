# telos-cli

Terminal interface for [telos-agent](..).

## Features

- **Single-prompt mode:** `telos "refactor lib.rs to use anyhow"` — one-shot agent invocation
- **Interactive REPL:** `telos chat` — multi-turn session with rustyline (history, tab completion, Emacs/Vi keybindings)
- **Slash commands:** `/exit`, `/help`, `/clear`, `/reset`, `/tools`, `/add`, `/drop`, `/model` inside the REPL
- **Markdown rendering:** Agent responses rendered through termimad for rich terminal output
- **Diff display:** File diffs colored with green/red ANSI escapes
- **Approval policies:** Configurable per-tool approval (AlwaysAllow, AlwaysAsk, AlwaysDeny) with interactive fallback
- **Config files:** User-level `~/.config/telos/config.toml` and project-level `.telos.toml` with layered merging
- **Session persistence:** Chat sessions auto-saved to `~/.local/share/telos/sessions/` or `<project>/.telos/sessions/`
- **Project detection:** Auto-discovers project root via `.git` or `.telos.toml` markers
- **Shell completions:** `telos completion bash|zsh`

## Build

From the workspace root:

```bash
cd /home/alin/codework/tiny_agent
cargo build -p telos-cli
```

## Install

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core/telos-cli
cargo install --path .
```

## Usage

### Single prompt

```bash
# DeepSeek
telos --provider deepseek --api-key $DEEPSEEK_API_KEY "Refactor src/lib.rs to use anyhow"

# Kimi
telos --provider kimi --api-key $MOONSHOT_API_KEY "Review src/lib.rs"

# Mock (for testing)
telos --provider mock "hello"
```

### Interactive chat

```bash
telos --provider deepseek chat
```

Inside the REPL:

```
telos> /help
Available commands:
  /exit, /quit  Exit the REPL
  /reset        Reset the conversation
  /clear        Clear the screen
  /tools        List available tools
  /help         Show this help
  /add <glob>   Add files matching a glob pattern
  /drop <glob>  Remove files matching a glob pattern
  /model <name> Change the active model

telos> Refactor the error handling in main.rs
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

[display]
theme = "dark"
render_markdown = true

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

## License

`telos-cli` is licensed under the MIT License. It includes code adapted from
OpenAI's Codex CLI, which is licensed under the Apache License, Version 2.0.
See the `NOTICE` file for details.
