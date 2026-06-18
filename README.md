# telos

**telos** 是一个 Rust 编写的意图驱动 agent runtime，封装「用户输入 → 模型采样 → 工具执行 → 结果回注」的完整 turn 循环。可作为 AI 编码助手、聊天机器人、自动化工作流的内核。

> Loop: intent → execute → think → complete

## 快速开始

### CLI

```bash
# 设置环境变量（推荐）
export DEEPSEEK_API_KEY=sk-...

# 全屏 TUI
telos

# 单次调用
telos "Review src/lib.rs"

# 指定 provider 和模型
telos --provider kimi --model kimi-k2-0711-preview "Refactor error handling"

# 生成 shell 补全
telos completion bash > /usr/share/bash-completion/completions/telos
```

API key 按以下优先级解析：`--api-key` 标志 → `DEEPSEEK_API_KEY` / `MOONSHOT_API_KEY` 环境变量 → 交互式输入。详细用法见 [cli/README.md](cli/README.md)。

### 库

```rust
use telos_agent::{
    AgentConfig, AgentSession, CompletionResponse, Message,
    MockProvider, StopReason, Tool, ToolContext, ToolDefinition,
    ToolOutput, ToolRegistry,
};

struct EchoTool;

#[async_trait::async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("echo", "Echo input text.", json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        }))
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
- 7 个 hook phase：`SessionStart`、`UserPromptSubmit`、`PreToolUse`、`PostToolUse`、`PostToolUseFailure`、`PostSampling`、`Stop`
- 取消检查点，可在 provider 调用或迭代间隙安全中断

### Provider
- 统一的 `ModelProvider` trait，内置 Kimi、DeepSeek、Mock 实现
- SSE streaming（基于 `async-openai`），指数退避重试
- `ErasedProvider` 辅助类型擦除

### 工具系统
- `Tool` trait + `ToolRegistry`，支持别名、JSON Schema 自动校验
- 内置工具：Bash、Read、Edit、Write、Glob、Grep、WebFetch、WebSearch、AskUserQuestion
- `SubagentTool`（完整子会话 + fork 并发多视角）、`SkillTool`（slash-command）
- Memory 工具（Read/Write/Grep/Edit/Status）、Task 工具（Create/Get/List/Update）
- MCP 工具桥接（stdio JSON-RPC，自动注册为 `mcp__<server>__<tool>`）
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
model = "deepseek-chat"
max_iterations = 16

[approval]
default_policy = "ask"

[approval.policies]
read = "allow"
shell = "ask"
write = "deny"
```

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
