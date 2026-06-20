# telos_agent Core API

`telos_agent` is the library crate. It is published to crates.io and documented
with rustdoc.

## Generate Docs

```bash
./scripts/generate-core-api-docs.sh
```

The generated entry point is:

```text
target/doc/telos_agent/index.html
```

## Main Entry Points

| Area | Public API | Use |
|---|---|---|
| Session runtime | `AgentSession`, `AgentConfig`, `TurnEvent`, `TurnResult` | Run blocking or streaming agent turns. |
| Providers | `ModelProvider`, `DeepSeekProvider`, `RoutedProvider`, `MockProvider` | Connect the runtime to LLM backends. |
| Tools | `Tool`, `ToolRegistry`, `ToolDefinition`, `ToolOutput`, `ToolContext` | Define and register callable capabilities. |
| Built-in tools | `register_core_tools`, `ShellTool`, `FileReadTool`, `FileWriteTool`, `FileEditTool`, `GlobTool`, `GrepTool`, `WebFetchTool`, `WebSearchTool` | Add default filesystem, shell, search, and web tools. |
| Human approval | `ApprovalHandler`, `ApprovalRequest`, `ApprovalDecision`, `PermissionEngine`, `PermissionRule` | Gate dangerous tool execution. |
| Persistence | `Storage`, `JsonlStorage`, `NoopStorage` | Save and resume sessions. |
| Prompt | `PromptAssembly`, `PromptSection`, built-in prompt sections | Build dynamic system prompts. |
| Memory | `MemoryStore`, `ProfileManager`, memory tools | Persist cross-session knowledge. |
| Tasks | `TaskManager`, `Task`, task tools | Track agent-visible tasks. |
| MCP | `McpManager`, `McpClient`, `McpToolBridge` | Bridge MCP tools into the registry. |
| Plugins | `PluginRegistry`, `PluginId`, `PluginError`, `BUILTIN_MARKETPLACE` | Load plugin-defined tools, skills, prompts, MCP servers, and subagents. |
| Subagents | `SubagentTool`, `SubagentRegistry`, `AgentDefinition`, `Synapse`, `ForkLens` | Run nested or forked agent work. |
| Diagnostics | `ToolDiagnosticsSink`, `JsonlToolDiagnosticsSink`, `ToolFailureEvent` | Record sanitized tool failures. |

## Minimal Runtime Flow

```rust
use telos_agent::{
    AgentConfig, AgentSession, DeepSeekConfig, DeepSeekProvider, ToolRegistry,
    register_core_tools,
};

# async fn example() -> Result<(), telos_agent::AgentError> {
let provider = DeepSeekProvider::new(DeepSeekConfig::from_env("deepseek-v4-pro")?);
let mut tools = ToolRegistry::new();
register_core_tools(&mut tools);

let mut session = AgentSession::new(AgentConfig::default())?;
let result = session.run_turn(&provider, &tools, "Summarize this project").await?;
println!("{}", result.final_message.text_content());
# Ok(())
# }
```

## Current Documentation Audit

`cargo rustdoc -p telos_agent -- -D missing_docs` currently reports many missing
field-level and internal public-item docs, especially in lower-level modules
such as bash security, diagnostics, code index, task tools, browser tools, and
tool schemas. The crate root and high-level public entry points are documented
and rustdoc generation succeeds, but full `missing_docs` enforcement should be
handled as a separate cleanup pass because it touches a large public surface.

