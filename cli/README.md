# telos-cli

Codex-style full-screen terminal interface for [telos-agent](..).

## Features

- **Full-screen TUI** — launch with `telos` for an immersive agent experience
- **Single-prompt mode** — `telos "refactor lib.rs"` for one-shot tasks
- **Context-aware** — auto-discovers `CLAUDE.md`, `AGENTS.md`, `CODEBUDDY.md`, `GEMINI.md` and git status
- **Streaming output** — real-time markdown rendering with tool-call indicators
- **Interactive approval** — approve/deny tool calls inline
- **Session persistence** — auto-saved to `.telos/sessions/`

## Usage

API key 通过环境变量传入（推荐），cli 也支持交互式输入和 `--api-key` 标志。
优先级：`--api-key` > 环境变量 > 交互式提示。

```bash
# 设置 API key
export DEEPSEEK_API_KEY=sk-...

# 全屏 TUI（默认 provider: mock）
telos

# 指定 provider
telos --provider deepseek

# 单次调用
telos "Review src/lib.rs"

# 指定模型
telos --provider kimi --model kimi-k2-0711-preview "Refactor error handling"

# 生成 shell 补全
telos completion bash > /usr/share/bash-completion/completions/telos
telos completion zsh  > /usr/local/share/zsh/site-functions/_telos
```

### Environment variables

| Variable | Flag | Description |
|----------|------|-------------|
| `TELOS_PROVIDER` | `--provider` | Model provider (kimi, deepseek, mock) |
| `TELOS_MODEL` | `--model` | Model name |
| `TELOS_API_KEY` | `--api-key` | API key (or use `DEEPSEEK_API_KEY` / `MOONSHOT_API_KEY`) |
| `TELOS_CWD` | `--cwd` | Working directory |

Provider-specific key env vars: `DEEPSEEK_API_KEY`, `MOONSHOT_API_KEY`.

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
```

Project config overrides user config for matching keys. CLI flags override both.

### Approval policies

| Policy | Behavior |
|--------|----------|
| `allow` | Auto-approve |
| `ask` | Prompt in TUI (default) |
| `deny` | Auto-deny |

Configured per-tool under `[approval.policies]`.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Alt+Enter` | Insert newline |
| `Ctrl+D` | Quit (empty input) |
| `Ctrl+C` | Cancel turn |
| `Ctrl+L` | Clear chat |
| `Ctrl+N` | New session |
| `PgUp` / `PgDn` | Scroll chat (page) |
| `↑` / `↓` | Scroll chat (line) |
| `a` / `y` | Approve tool call |
| `d` / `n` | Deny tool call |
| `e` | Edit request |

## License

MIT
