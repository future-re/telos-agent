use crate::error::AgentError;
use crate::provider::ModelProvider;
use std::sync::Arc;

/// Execute a prompt-type hook using a lightweight model.
pub async fn exec_prompt_hook(
    provider: &Arc<dyn ModelProvider + Send + Sync>,
    prompt: &str,
) -> Result<String, AgentError> {
    let messages = vec![crate::message::Message::user(prompt)];
    let request = crate::provider::CompletionRequest {
        system_prompt: Some("You are a short assistant. Respond concisely.".into()),
        system_prompt_blocks: None,
        messages,
        tools: vec![],
    };
    let response = provider.complete(request).await?;
    Ok(response.message.text_content())
}
