use async_trait::async_trait;
use serde_json::{Value, json};
use tiny_agent_core::{
    AgentConfig, AgentError, AgentSession, AnthropicConfig, AnthropicProvider, Tool, ToolContext,
    ToolDefinition, ToolOutput, ToolRegistry,
};

struct EchoJsonTool;

#[async_trait]
impl Tool for EchoJsonTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo_json".into(),
            description: "Echo JSON input back to the model.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                },
                "required": ["value"]
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
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Call echo_json with value='hello from tool' and summarize the result.".into());

    let config = AnthropicConfig::from_env("claude-sonnet-4-5", 1024)?;
    let provider = AnthropicProvider::new(config);

    let mut tools = ToolRegistry::new();
    tools.register(EchoJsonTool);

    let mut session = AgentSession::new(AgentConfig {
        system_prompt: Some("You are a concise coding agent.".into()),
        max_iterations: 6,
        ..AgentConfig::default()
    });

    let result = session.run_turn(&provider, &tools, prompt).await?;
    println!("{}", result.final_message.text_content());
    Ok(())
}
