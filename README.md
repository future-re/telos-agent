# telos

[![License](https://img.shields.io/github/license/future-re/telos-agent?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-workspace-orange?style=flat-square&logo=rust)](Cargo.toml)
[![CLI](https://img.shields.io/badge/CLI-telos-blue?style=flat-square)](cli/README.md)
[![Last commit](https://img.shields.io/github/last-commit/future-re/telos-agent?style=flat-square)](https://github.com/future-re/telos-agent/commits/main)
[![Repository](https://img.shields.io/badge/GitHub-future--re%2Ftelos--agent-24292f?style=flat-square&logo=github)](https://github.com/future-re/telos-agent)

**telos** 是一个 Rust 编写的意图驱动 agent runtime，封装「用户输入 -> 模型采样 -> 工具执行 -> 结果回注」的完整 turn 循环。它可以作为 AI 编码助手、聊天机器人和自动化工作流的内核。

> Loop: intent -> execute -> think -> complete

| 模块 | 说明 |
| --- | --- |
| [`telos_agent`](core/README.md) | 核心 runtime、provider、工具系统、权限、memory、MCP、storage |
| [`telos-cli`](cli/README.md) | Codex-style 全屏 TUI 和命令行客户端 |
| `desktop/` | 基于同一 runtime 的桌面端壳层 |
| [`site/`](site/) | 在线引导与项目站点 |

## 目录

- [仓库动态](#仓库动态)
- [在线引导](#在线引导)
- [快速开始](#快速开始)
- [功能概览](#功能概览)
- [架构](#架构)
- [执行流程](#执行流程)
- [核心对象](#核心对象)
- [配置](#配置)
- [边界](#边界)
- [开发](#开发)
- [License](#license)

## 仓库动态

最新提交快照：

| Commit | Date | Summary |
| --- | --- | --- |
| [`aa86490`](https://github.com/future-re/telos-agent/commit/aa86490) | 2026-06-20 | Fix TUI shortcut and paste handling |
| [`6f86ce6`](https://github.com/future-re/telos-agent/commit/6f86ce6) | 2026-06-20 | feat: update dependencies and improve input handling in CLI |
| [`7c4bed0`](https://github.com/future-re/telos-agent/commit/7c4bed0) | 2026-06-20 | feat: inject runtime input during tool turns |
| [`4f6cd77`](https://github.com/future-re/telos-agent/commit/4f6cd77) | 2026-06-20 | style: clarify inline approval panel |
| [`7f04b8c`](https://github.com/future-re/telos-agent/commit/7f04b8c) | 2026-06-20 | fix: show inline approval action hints |
| [`7854856`](https://github.com/future-re/telos-agent/commit/7854856) | 2026-06-20 | fix: update .gitignore and format tsconfig.json for consistency |

更多历史见 [GitHub commits](https://github.com/future-re/telos-agent/commits/main)。

## 在线引导

第一次使用 telos 时，推荐先打开在线引导页：

<https://future-re.github.io/telos-agent/>

该页面会引导你完成安装、配置 provider/API key，并启动 CLI 或 TUI。

## 快速开始

### 安装 CLI

```bash
# 从 PyPI 安装（推荐给 Python 用户）
pip install telos-cli

# 或从 crates.io 安装
cargo install telos-cli

# 查看命令帮助
telos --help
```

### 运行 telos

```bash
# 设置环境变量（推荐）
export DEEPSEEK_API_KEY=sk-...

# 全屏 TUI
telos

# 单次调用
telos "Review src/lib.rs"

# 指定 provider 和模型
telos --provider deepseek --model deepseek-v4-pro "Refactor error handling"

# 使用双模型路由：thinking 负责规划/恢复，fast 负责工具执行
telos --provider deepseek \
  --thinking-model deepseek-v4-pro \
  --fast-model deepseek-v4-flash \
  "Refactor error handling"

# 生成 shell 补全
telos completion bash > /usr/share/bash-completion/completions/telos
```

当前 CLI 支持 `deepseek` 和 `mock` provider。API key 按以下优先级解析：`--api-key` 标志 -> `DEEPSEEK_API_KEY` 环境变量 -> 配置文件 `[env]` -> 交互式输入。

详细用法见 [cli/README.md](cli/README.md)。

### 作为库使用

```bash
cargo add telos_agent
```

```rust
use serde_json::{json, Value};
use telos_agent::{
    AgentConfig, AgentError, AgentSession, CompletionResponse, Message, MockProvider,
    StopReason, Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry,
};

struct EchoTool;

#[async_trait::async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".into(),
            description: "Echo input text.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
        }
    }

    async fn invoke(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput { content: json!({ "echo": args }) })
    }
}

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    let provider = MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("done"),
        stop_reason: StopReason::EndTurn,
        usage: None,
    }]);

    let mut tools = ToolRegistry::new();
    tools.register(EchoTool);

    let mut session = AgentSession::new(AgentConfig {
        base_system_prompt: Some("You are a concise assistant.".into()),
        ..Default::default()
    })?;

    let result = session.run_turn(&provider, &tools, "hello").await?;
    println!("{}", result.final_message.text_content());
    Ok(())
}
```

## 功能概览

### Core Runtime

- `AgentSession` 驱动 turn 循环：采样 -> 工具执行 -> 结果回注。
- `TurnEvent` 流式事件暴露采样、工具、compaction、停止等阶段。
- Hook phase 当前在 turn loop 内执行 `PostSampling` 和 `Stop`，hook 注册表保留扩展接口。
- 取消检查点可在 provider 调用或迭代间隙安全中断。

### Provider

- 统一的 `ModelProvider` trait，内置 DeepSeek、双模型 `RoutedProvider`、Mock 实现。
- DeepSeek 使用原生 Chat Completions / SSE streaming 请求层，支持 DeepSeek `thinking`、准确 usage 明细和指数退避重试。
- `ModelHint` 将请求标记为 thinking、execution、recovery 或 summarization；DeepSeek 仅在 `Thinking` / `Recovery` 请求中开启 thinking，并把 `reasoning_content` 作为 thinking 输出流式展示。
- Provider 返回的 `usage` 是准确用量语义，包含输入/输出 token、总 token、prompt cache hit/miss 和 reasoning token（如 API 返回）。
- `ErasedProvider` 辅助类型擦除。

### DeepSeek API 支持范围

- 已支持：Chat Completions、SSE streaming、tool calls、JSON Output、Beta Prefix Completion、Beta FIM Completion、模型列表、余额查询、`thinking`、`reasoning_content`、`stream_options.include_usage`、usage 明细、400/401/402/422/429/500/503 错误语义和 retry 判断。
- Context Caching 在 DeepSeek API 侧默认生效；当前通过 provider usage 暴露 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`，不额外伪造管理接口。
- 当前中文 API 文档未看到 Batch 端点，因此未封装 Batch。

### 工具系统

- `Tool` trait + `ToolRegistry`，支持别名、JSON Schema 自动校验。
- 内置工具：默认 Shell（Linux/macOS 为 Bash，Windows 为 PowerShell，可配置覆盖）、Read、Edit、Write、Glob、Grep、WebFetch、WebSearch、AskUserQuestion、Browser、CodeIndex。
- `SubagentTool`（完整子会话 + fork 并发多视角）、`SkillTool`（slash-command）。
- Memory 工具（Read/Write/Grep/Edit/Status）、Task 工具（Create/Get/List/Update）。
- MCP 工具桥接（stdio JSON-RPC，自动注册为 `mcp__<server>__<tool>`）。
- 插件系统可加载 manifest、tool specs、skills、prompt sections 和 MCP 声明；core 层已提供 registry/apply 能力。
- 工具超时、panic 隔离、文件写冲突保护。

### 权限与安全

- `PermissionEngine` 规则引擎（通配、command_prefix、cwd_prefix）。
- Bash AST 安全分析（fail-closed）。
- `ApprovalHandler` 人机审批（Allow / Deny / Modify）。

### Prompt / Skills / Memory

- 动态 system prompt 组装（`PromptAssembly` + identity、tools、cwd、date、skills、git status、memory、MCP、task guidance 等内置 section）。
- Markdown skill 系统（YAML frontmatter -> prompt 注入）。
- 跨会话持久记忆（`MemoryStore`）+ 上下文画像（`ProfileManager`）。

### 存储与会话管理

- `JsonlStorage` JSONL 持久化，`NoopStorage` 用于无持久化场景。
- Token 预算感知 compaction（`SummaryCompaction` + 字符截断）。
- 任务管理系统（`TaskManager`，支持 blocked_by/blocks 依赖）。

### CLI / TUI

- `telos` 无 prompt 时启动全屏 TUI，`telos "..."` 执行单次调用，`telos chat` 进入交互会话。
- TUI 使用后台 agent task 消费 `TurnEvent`，支持流式 markdown、工具状态、审批 overlay、自动审批、会话保存/恢复和模型切换。
- CLI 启动时加载项目上下文、memory runtime，并注册轻量 CodeIndex 工具用于代码搜索和行号定位。

## 架构

| 层 | 职责 |
| --- | --- |
| **Session** | `AgentSession` 持有消息历史、配置、文件读状态，暴露 `run_turn` / `run_turn_stream` |
| **Runtime** | 单轮 turn 内的迭代循环、provider 调用、compaction、hook、工具编排、持久化 |
| **Provider** | `ModelProvider` 统一封装不同 LLM 后端，流式输出统一为 `ProviderEvent` |
| **Tool** | `Tool` trait + 执行器：参数校验、权限判定、审批、调用、结果格式化 |
| **Prompt** | `PromptAssembly` 动态组装 system prompt，缓存静态 section |
| **Permissions** | `PermissionEngine` 规则引擎 + bash AST 分析 + `ApprovalHandler` 审批 |
| **MCP** | `McpManager` + `McpToolBridge`，stdio JSON-RPC 接入 MCP 生态 |
| **Fork** | `Synapse` + `ForkLens`，轻量级上下文分叉，多视角并发执行 |
| **Storage** | `Storage` trait -> JSONL / Noop 后端，含回滚与恢复 |

## 执行流程

一次 `run_turn` 内部按以下流水线执行：

1. **准备输入**：CLI/TUI 收集用户输入、项目上下文、配置、memory 和可用工具。
2. **Prompt 构建**：`PromptAssembly` 组装 identity、tools、cwd、date、skills、git status、memory、task guidance 等 section；未配置时使用 `base_system_prompt`。
3. **预算检查**：根据 `TokenBudget` 和 `CompactionStrategy` 判断是否需要压缩历史；超长 tool result 会按 `max_tool_result_chars` 截断。
4. **Provider 采样**：通过 `ModelProvider` 调用 DeepSeek、双模型路由或 Mock provider；流式调用会产出 `ProviderEvent`，并按 `ModelHint` 区分 thinking、execution、recovery、summarization。
5. **Hook 阶段**：assistant message 进入历史后触发 `PostSampling` hook，便于扩展审计、指标或自定义流程。
6. **工具判定**：如果没有 tool call，进入停止阶段；否则逐条进入工具执行流水线。
7. **工具执行**：先执行工具级校验和 JSON Schema 校验，再通过 `PermissionEngine`、工具自身 `check_permission` 和 `ApprovalHandler` 判定权限；随后在超时、panic 隔离、文件写冲突保护下调用工具。
8. **并发编排**：只读或声明为 concurrency-safe 的工具按 `tool_concurrency_limit` 分批并发执行，结果按原始 tool call 顺序回注。
9. **结果回注**：工具输出写回为 `Role::Tool` 消息，模型进入下一次采样，直到得到最终回复或触达迭代上限。
10. **收尾持久化**：触发 `Stop` hook，汇总 `TurnResult`、usage、metrics 和错误信息；配置了 `Storage` 时写入 JSONL 会话记录。

## 核心对象

| 类型 / Trait | 职责 |
| --- | --- |
| `AgentSession` | 保存消息历史和运行状态，驱动 `run_turn` / `run_turn_stream`。 |
| `AgentConfig` | 配置 prompt、cwd、env、权限、审批、storage、compaction、token budget、hooks、插件、skills 和取消状态。 |
| `ModelProvider` | Provider 抽象，统一非流式 `complete` 和流式 `stream_complete`。 |
| `DeepSeekProvider` / `RoutedProvider` / `MockProvider` | 内置 provider：真实 DeepSeek、双模型路由和测试用 mock。 |
| `CompletionRequest` / `CompletionResponse` / `ProviderEvent` | Provider 的输入、输出和流式事件模型。 |
| `Tool` / `ToolRegistry` | 工具接口和注册表，负责 definition、alias、校验、权限和调用。 |
| `ToolContext` / `ToolProgress` | 工具调用上下文和长任务进度事件。 |
| `PermissionEngine` / `PermissionRule` / `ApprovalHandler` | 规则权限、人机审批和工具调用 gating。 |
| `PromptAssembly` / `PromptSection` | system prompt 动态装配和静态 section 缓存。 |
| `SkillRegistry` / `SkillLoader` / `SkillTool` | Markdown skill 的加载、注册和工具化调用。 |
| `MemoryStore` / `ProfileManager` | 跨会话记忆和用户/项目画像管理。 |
| `McpManager` / `McpToolBridge` | stdio MCP server 接入和工具桥接。 |
| `PluginRegistry` / `PluginPromptSection` | 插件 manifest、工具、skills、prompt section 和 marketplace 应用。 |
| `SubagentTool` / `Synapse` / `ForkLens` | 子 agent 工具和并发多视角执行。 |
| `Storage` / `JsonlStorage` / `NoopStorage` | 会话持久化、恢复和无持久化后端。 |
| `TurnEvent` / `TurnResult` / `SessionMetrics` | UI/API 消费的事件流、最终结果和运行指标。 |
| `Message` / `ContentBlock` / `ThinkingBlock` / `ToolCall` / `ToolResult` | 模型、工具和 runtime 共享的消息结构。 |

## 配置

支持用户级 `~/.config/telos/config.toml` 和项目级 `.telos.toml`，项目配置覆盖用户配置：

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

[diagnostics]
# CLI 默认启用本地工具失败诊断，写入 .telos/diagnostics/
enabled = true
retention_days = 14

[diagnostics.github]
# GitHub issue 上报默认关闭；开启后需要 GITHUB_TOKEN
enabled = false
repository = "future-re/telos-agent"
interval_hours = 24
min_occurrences = 3
```

诊断日志只保存脱敏后的工具失败摘要，用于本地分析；不会保存原始命令、完整 stdout/stderr、环境变量值、模型消息或会话转录。开启 `[diagnostics.github].enabled = true` 后，CLI 会按间隔把重复失败聚合成隐私清洗后的 GitHub Issue。

## 边界

当前仓库聚焦本地 agent runtime、CLI/TUI、桌面壳层和文档站点。以下能力不属于当前稳定承诺：

- 多模态输入输出。
- 跨 provider 自动 fallback。
- 远程沙箱或容器级隔离；当前提供规则权限、命令安全分析和人工审批。
- 长期兼容的外部插件市场协议；本仓库已有本地插件 registry 和 manifest 能力，但仍在演进。

## 开发

```bash
# 全量测试
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets

# 安装 CLI
cd cli && cargo install --path .
```

### 运行 desktop

```bash
# 启动桌面端开发模式
cd desktop
npm run tauri dev
```

如果只需要调试前端界面，不启动 Tauri 壳层：

```bash
cd desktop
npm run dev
```

默认前端开发地址为 `http://127.0.0.1:1420`。

### 构建 desktop

```bash
cd desktop
npm run build
```

## License

MIT
