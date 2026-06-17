# tiny_agent_core

`tiny_agent_core` 是一个用 Rust 编写的轻量级 agent runtime，重点覆盖会话管理、模型调用、工具执行和结果回注这一条核心链路。

它主要面向两类使用场景：

- 作为实验底座，用于验证 agent loop、tool use 和 provider adapter 的设计
- 作为运行时内核，供 TUI、HTTP 服务、任务系统或其他编排层集成

## 已实现能力

- 会话级 `AgentSession`
- 通用 `ModelProvider` 抽象
- `KimiProvider`
- `DeepSeekProvider`
- 工具注册、参数校验、权限判断和执行结果回注
- `TurnEvent` 事件流
- `HookRegistry`，支持 `post_sampling` 和 `stop` 两个 hook phase
- tool result 压缩
- provider streaming 事件抽象，非流式 provider 可通过默认实现兼容
- 基于 async-openai 的 SSE streaming
- 基础工具执行编排，支持并发安全工具分批执行和实时 tool progress
- `TokenBudget` + auto compact 触发
- `SubagentTool`，支持 in-process 子 agent
- JSONL snapshot 存储与会话恢复
- 内置核心工具：shell、file_read、file_write、file_edit、glob、grep
- `MockProvider`，用于测试和样例驱动开发

## 暂不包含

- UI / TUI / Web 层
- MCP / plugin / bridge / swarm
- classifier / sandbox 等复杂权限审批流程
- 多模态、thinking block 和 provider 级 fallback/retry

## 核心对象

- `AgentSession`: 保存消息历史，并驱动每一轮 turn
- `AgentConfig`: 会话配置，包括 `system_prompt`、`max_iterations`、`cwd`、`env`
- `ModelProvider`: 模型适配接口，接收消息和工具定义，返回 assistant message
- `Tool`: 工具接口，覆盖 definition、validate、permission 和 invoke
- `register_core_tools`: 注册内置 shell / 文件 / 搜索工具
- `SubagentTool`: 将一个 in-process 子 agent 注册为工具
- `TokenBudget`: 基于估算 token 触发 compact / budget exceeded 事件
- `TurnEvent`: turn 执行过程中产生的结构化事件
- `Hook`: 在采样后或停止时插入自定义逻辑

## 执行流程

1. 将用户输入追加到当前会话的消息历史
2. runtime 调用 provider 完成一次模型采样
3. 如果 assistant 返回 tool call，则按并发安全性分批执行对应工具
4. 将 tool result 作为 `Role::Tool` 消息写回会话
5. 进入下一轮迭代，直到模型停止或达到 `max_iterations`

## 最小示例

```rust
use async_trait::async_trait;
use serde_json::{json, Value};
use tiny_agent_core::{
    AgentConfig, AgentError, AgentSession, CompletionResponse, Message,
    MockProvider, StopReason, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolRegistry,
};

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".into(),
            description: "Echo input text.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput {
            content: json!({ "echo": arguments }),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    let provider = MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("done"),
        stop_reason: StopReason::EndTurn,
    }]);

    let mut tools = ToolRegistry::new();
    tools.register(EchoTool);

    let mut session = AgentSession::new(AgentConfig {
        system_prompt: Some("You are a concise assistant.".into()),
        ..AgentConfig::default()
    });

    let result = session.run_turn(&provider, &tools, "hello").await?;
    println!("{}", result.final_message.text_content());
    Ok(())
}
```

## 运行示例

仓库中提供了一个基于真实 provider 的工具调用示例：

```bash
export MOONSHOT_API_KEY=...
cargo run --example kimi_tool_loop -- "Use the echo_json tool once, then summarize."
```

## 测试

```bash
cargo test
```
