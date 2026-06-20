# telos

**telos** 是一个 Rust 编写的意图驱动 agent runtime，封装「用户输入 → 模型采样 → 工具执行 → 结果回注」的完整 turn 循环。可作为 AI 编码助手、聊天机器人、自动化工作流的内核。

> Loop: intent → execute → think → complete

Repository: <https://github.com/future-re/telos-agent>

## 快速开始

### CLI

```bash
# 从 crates.io 安装
cargo install telos-cli

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

当前 CLI 支持 `deepseek` 和 `mock` provider。API key 按以下优先级解析：`--api-key` 标志 → `DEEPSEEK_API_KEY` 环境变量 → 配置文件 `[env]` → 交互式输入。详细用法见 [cli/README.md](cli/README.md)。

### 库

```bash
cargo add telos_agent
```

```rust
use telos_agent::{
    AgentConfig, AgentError, AgentSession, CompletionResponse, Message, MockProvider,
    StopReason, Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry,
};
use serde_json::{json, Value};

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

### 核心循环
- `AgentSession` 驱动 turn 循环（采样 → 工具执行 → 结果回注）
- `TurnEvent` 流式事件，暴露采样、工具、compaction、停止等阶段
- hook phase：当前 turn loop 内执行 `PostSampling` 和 `Stop`，hook 注册表保留扩展接口
- 取消检查点，可在 provider 调用或迭代间隙安全中断

### Provider
- 统一的 `ModelProvider` trait，内置 DeepSeek、双模型 `RoutedProvider`、Mock 实现
- DeepSeek 使用原生 Chat Completions / SSE streaming 请求层，支持 DeepSeek `thinking`、准确 usage 明细和指数退避重试
- `ModelHint` 将请求标记为 thinking、execution、recovery 或 summarization；DeepSeek 仅在 `Thinking` / `Recovery` 请求中开启 thinking，并把 `reasoning_content` 作为 thinking 输出流式展示
- Provider 返回的 `usage` 是准确用量语义，包含输入/输出 token、总 token、prompt cache hit/miss 和 reasoning token（如 API 返回）
- `ErasedProvider` 辅助类型擦除

### DeepSeek API 支持范围
- 已支持：Chat Completions、SSE streaming、tool calls、JSON Output、Beta Prefix Completion、Beta FIM Completion、模型列表、余额查询、`thinking`、`reasoning_content`、`stream_options.include_usage`、usage 明细、400/401/402/422/429/500/503 错误语义和 retry 判断
- Context Caching 在 DeepSeek API 侧默认生效；当前通过 provider usage 暴露 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`，不额外伪造管理接口
- 当前中文 API 文档未看到 Batch 端点，因此未封装 Batch

### 工具系统
- `Tool` trait + `ToolRegistry`，支持别名、JSON Schema 自动校验
- 内置工具：Bash、Read、Edit、Write、Glob、Grep、WebFetch、WebSearch、AskUserQuestion
- `SubagentTool`（完整子会话 + fork 并发多视角）、`SkillTool`（slash-command）
- Memory 工具（Read/Write/Grep/Edit/Status）、Task 工具（Create/Get/List/Update）
- MCP 工具桥接（stdio JSON-RPC，自动注册为 `mcp__<server>__<tool>`）
- 插件系统可加载 manifest、tool specs、skills、prompt sections 和 MCP 声明；core 层已提供 registry/apply 能力
- 工具超时、panic 隔离、文件写冲突保护

### 权限与安全
- `PermissionEngine` 规则引擎（通配、command_prefix、cwd_prefix）
- Bash AST 安全分析（fail-closed）
- `ApprovalHandler` 人机审批（Allow / Deny / Modify）

### Prompt / Skills / Memory
- 动态 system prompt 组装（`PromptAssembly` + 14 个内置 section）
- Markdown skill 系统（YAML frontmatter → prompt 注入）
- 跨会话持久记忆（`MemoryStore`）+ 上下文画像（`ProfileManager`）

### 存储与会话管理
- `JsonlStorage` JSONL 持久化，`NoopStorage` 用于无持久化场景
- Token 预算感知 compaction（`SummaryCompaction` + 字符截断）
- 任务管理系统（`TaskManager`，支持 blocked_by/blocks 依赖）

### CLI / TUI
- `telos` 无 prompt 时启动全屏 TUI，`telos "..."` 执行单次调用，`telos chat` 进入交互会话
- TUI 使用后台 agent task 消费 `TurnEvent`，支持流式 markdown、工具状态、审批 overlay、自动审批、会话保存/恢复和模型切换
- CLI 启动时加载项目上下文、memory runtime，并注册轻量 CodeIndex 工具用于代码搜索和行号定位

## 架构

| 层 | 职责 |
|---|---|
| **Session** | `AgentSession` 持有消息历史、配置、文件读状态，暴露 `run_turn` / `run_turn_stream` |
| **Runtime** | 单轮 turn 内的迭代循环、provider 调用、compaction、hook、工具编排、持久化 |
| **Provider** | `ModelProvider` 统一封装不同 LLM 后端，流式输出统一为 `ProviderEvent` |
| **Tool** | `Tool` trait + 执行器：参数校验、权限判定、审批、调用、结果格式化 |
| **Prompt** | `PromptAssembly` 动态组装 system prompt，缓存静态 section |
| **Permissions** | `PermissionEngine` 规则引擎 + bash AST 分析 + `ApprovalHandler` 审批 |
| **MCP** | `McpManager` + `McpToolBridge`，stdio JSON-RPC 接入 MCP 生态 |
| **Fork** | `Synapse` + `ForkLens`，轻量级上下文分叉，多视角并发执行 |
| **Storage** | `Storage` trait → JSONL / Noop 后端，含回滚与恢复 |

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

## 开发

```bash
# 全量测试
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets

# 安装 CLI
cd cli && cargo install --path .
```

## License

MIT
