mod common;

use serde_json::json;
use std::sync::Arc;

use telos_agent::*;

#[test]
fn subagent_tool_runs_in_process_agent() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let outer_provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "subagent".into(),
                        arguments: json!({ "prompt": "solve inside" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("outer done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let inner_provider = Arc::new(MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("inner answer"),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }]));
        let mut tools = ToolRegistry::new();
        tools.register(SubagentTool::new(
            inner_provider,
            ToolRegistry::new(),
            AgentConfig::default(),
        ));
        let mut session = AgentSession::new(AgentConfig {
            approval_handler: Some(Arc::new(FixedDecisionHandler {
                decision: ApprovalDecision::Allow,
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&outer_provider, &tools, "delegate").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("inner answer"));
    });
}

#[test]
fn subagent_fork_mode_runs_multiple_lenses() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let outer_provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "subagent".into(),
                        arguments: json!({
                            "prompt": "analyze",
                            "mode": "fork",
                            "forks": [
                                {
                                    "lens": "security",
                                    "system_prompt": "You are a security expert",
                                    "task": "Find security issues"
                                },
                                {
                                    "lens": "performance",
                                    "system_prompt": "You are a performance expert",
                                    "task": "Find perf issues"
                                }
                            ]
                        }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("outer done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let inner_provider = Arc::new(MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("Security: found XSS"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("Performance: slow query"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(SubagentTool::new(
            inner_provider,
            ToolRegistry::new(),
            AgentConfig::default(),
        ));
        let mut session = AgentSession::new(AgentConfig {
            approval_handler: Some(Arc::new(FixedDecisionHandler {
                decision: ApprovalDecision::Allow,
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&outer_provider, &tools, "analyze code").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        let text = tool_result.text();
        assert!(text.contains("Security: found XSS"), "expected security result, got: {text}");
        assert!(
            text.contains("Performance: slow query"),
            "expected performance result, got: {text}"
        );
    });
}

#[tokio::test]
async fn plugin_tool_integration() {
    use telos_agent::{
        AgentConfig,
        hooks::HookRegistry,
        mcp::McpManager,
        plugin::{PluginId, PluginRegistry},
        prompt::PromptAssembly,
        skills::SkillRegistry,
        tool::ToolRegistry,
    };
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("mytool@test");
    std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();

    // Write plugin.json
    let manifest = serde_json::json!({
        "name": "mytool",
        "version": "1.0.0",
        "tools": ["./tools/uppercase.json"]
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Write a tool spec
    let tool_spec = serde_json::json!({
        "name": "uppercase",
        "description": "Converts text to uppercase using tr",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        },
        "command": "tr",
        "args": ["[:lower:]", "[:upper:]"],
        "permission": "allow",
        "isConcurrencySafe": true
    });
    std::fs::write(
        plugin_dir.join("tools").join("uppercase.json"),
        serde_json::to_string_pretty(&tool_spec).unwrap(),
    )
    .unwrap();

    // Register and enable the plugin
    let mut registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id = PluginId::parse("mytool@test").unwrap();
    registry.enable(&id).unwrap();

    // Apply plugins via AgentConfig
    let tools = ToolRegistry::new();
    let hooks = HookRegistry::new();
    let skills = SkillRegistry::new();
    let mcp = McpManager::new(std::collections::HashMap::new());
    let prompt = PromptAssembly::new();

    let config = AgentConfig {
        plugin_registry: Some(std::sync::Arc::new(registry)),
        ..AgentConfig::default()
    };

    let (tools, _hooks, _skills, _mcp, _prompt) =
        config.apply_plugins(tools, hooks, skills, mcp, prompt).unwrap();

    // Verify the tool is registered with namespace
    let tool = tools.get("plugin__mytool__uppercase");
    assert!(tool.is_ok(), "plugin tool should be registered: {:?}", tool.err());
}
