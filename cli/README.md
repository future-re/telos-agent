# telos-cli

Codex-style full-screen terminal interface for [telos-agent](..).

## Features

- **Full-screen TUI** — launch with `telos` for an immersive agent experience
- **Single-prompt mode** — `telos "refactor lib.rs"` for one-shot tasks
- **Context-aware** — auto-discovers `CLAUDE.md`, `AGENTS.md`, `CODEBUDDY.md`, `GEMINI.md` and git status
- **Streaming output** — real-time markdown rendering with tool-call indicators
- **Interactive approval** — approve/deny tool calls inline
- **Auto mode** — toggle automatic approval from the TUI and persist it in config
- **Dual-model routing** — use a thinking model for planning/recovery and a fast model for execution
- **Memory and CodeQL integration** — project memory is registered by default; CodeQL can be enabled from config
- **Session persistence** — auto-saved to `.telos/sessions/`

## Usage

API key 通过环境变量传入（推荐），cli 也支持交互式输入和 `--api-key` 标志。
优先级：`--api-key` > provider-specific 环境变量 > 配置文件 `[env]` > 交互式提示。

```bash
# 设置 API key
export DEEPSEEK_API_KEY=sk-...

# 全屏 TUI（未配置 provider 时会启动 onboarding；非交互环境默认 mock）
telos

# 指定 provider
telos --provider deepseek

# 单次调用
telos "Review src/lib.rs"

# 指定模型
telos --provider deepseek --model deepseek-v4-pro "Refactor error handling"

# 指定双模型路由
telos --provider deepseek \
  --thinking-model deepseek-v4-pro \
  --fast-model deepseek-v4-flash \
  "Refactor error handling"

# 生成 shell 补全
telos completion bash > /usr/share/bash-completion/completions/telos
telos completion zsh  > /usr/local/share/zsh/site-functions/_telos
```

### Environment variables

| Variable | Flag | Description |
|----------|------|-------------|
| `TELOS_PROVIDER` | `--provider` | Model provider (`deepseek`, `mock`) |
| `TELOS_MODEL` | `--model` | Fallback model for any path without an explicit thinking/fast model |
| `TELOS_THINKING_MODEL` | `--thinking-model` | Model for planning, first iteration, recovery, and periodic rethink |
| `TELOS_FAST_MODEL` | `--fast-model` | Model for tool execution and routine follow-up iterations |
| `TELOS_API_KEY` | `--api-key` | Provider API key |
| `TELOS_CWD` | `--cwd` | Working directory |
| `TELOS_CONFIG` | `--config` | Explicit config file path |

Provider-specific key env vars: `DEEPSEEK_API_KEY`.

### Config files

**User config** (`~/.config/telos/config.toml`):

```toml
[agent]
provider = "deepseek"
max_iterations = 16

[agent.models]
thinking = "deepseek-v4-pro"
fast = "deepseek-v4-flash"

[env]
DEEPSEEK_API_KEY = "sk-..."

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
model = "deepseek-v4-pro"
max_iterations = 32
```

Project config overrides user config for matching keys. CLI flags override both.

### Provider and model resolution

`--provider` / `TELOS_PROVIDER` / `[agent].provider` accepts `deepseek`, `deep`, or `mock`.

`--thinking-model` / `TELOS_THINKING_MODEL` / `[agent.models].thinking` controls the thinking path. `--fast-model` / `TELOS_FAST_MODEL` / `[agent.models].fast` controls the fast path. `--model` / `TELOS_MODEL` / `[agent].model` is used as the fallback for either path when that path is not set explicitly.

If the resolved thinking and fast models differ, CLI builds a routed provider:

```toml
[agent]
provider = "deepseek"

[agent.models]
thinking = "deepseek-v4-pro"
fast = "deepseek-v4-flash"
```

Without explicit model settings, DeepSeek defaults to `deepseek-v4-pro` for thinking and `deepseek-v4-flash` for fast execution.

### CodeQL

CodeQL is runtime-gated and disabled unless configured. When enabled, the CLI registers the `CodeQL` tool, injects a CodeQL prompt section, and runs startup analysis in the background.

```toml
[codeql]
enabled = true
language = "rust"
query_packs = ["security-and-quality"]
max_results = 50
timeout_secs = 120
```

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
| `Shift+Tab` | Toggle auto mode |
| `/` commands | Open command popups such as api/model/session/tool selectors |
| `PgUp` / `PgDn` | Scroll chat (page) |
| `↑` / `↓` | Scroll chat (line) |
| `a` / `y` | Approve tool call |
| `d` / `n` | Deny tool call |
| `e` | Edit request |

## License

MIT
