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
        system_prompt_blocks: vec![crate::prompt::PromptBlock::dynamic(
            "prompt_hook",
            "You are a short assistant. Respond concisely.",
        )],
        messages,
        tools: vec![],
        model_hint: None,
        max_tokens: None,
    };
    let response = provider.complete(request).await?;
    Ok(response.message.text_content())
}
