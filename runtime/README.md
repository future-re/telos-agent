# telos_agent

`telos_agent` is the core Rust runtime for telos. It provides the agent turn
loop, streaming events, model provider abstraction, tool execution, permissions,
memory, MCP integration, skills, plugins, and session storage.

Repository: <https://github.com/future-re/telos-agent>

## Install

```bash
cargo add telos_agent
```

For the terminal app, install the CLI crate instead:

```bash
cargo install telos-cli
```

## Quick Start

```rust
use serde_json::{json, Value};
use std::sync::Arc;
use telos_agent::{
    AgentConfig, AgentError, AgentRuntime, CompletionResponse, Message, MockProvider,
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
    let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("done"),
        stop_reason: StopReason::EndTurn,
        usage: None,
    }]));

    let mut tools = ToolRegistry::new();
    tools.register(EchoTool);

    let runtime = AgentRuntime::new(AgentConfig {
        base_system_prompt: Some("You are a concise assistant.".into()),
        ..Default::default()
    }, provider, tools)?;
    let session = runtime.create_session().await?;

    let result = runtime.run_turn(&session, "hello").await?;
    println!("{}", result.final_message.text_content());
    Ok(())
}
```

## Runtime Surface

- `AgentRuntime` owns providers and tools; `AgentSession` owns conversation state.
- `TurnEvent` exposes streaming assistant text, thinking text, tool calls,
  progress, usage, retries, and turn completion.
- `ModelProvider` abstracts LLM backends. The crate includes DeepSeek, routed
  dual-model, erased-provider, and mock providers.
- `Tool` and `ToolRegistry` provide pluggable tools with JSON Schema validation.
- Built-in tools include filesystem, shell, search, web, memory, tasks, MCP,
  skills, and subagents.
- `ApprovalHandler`, `PermissionEngine`, and bash safety analysis support
  human-in-the-loop execution.
- `JsonlStorage` and `NoopStorage` support persisted and ephemeral sessions.

## Package Layout

- `telos_agent`: core runtime library.
- `telos_agent_host`: shared configuration and assembly for application hosts.
- `telos-cli`: terminal UI and command-line client built on `telos_agent`.
- Desktop builds are distributed separately as native app packages.

## License

MIT
