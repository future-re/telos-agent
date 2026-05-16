use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    #[error("tool `{tool}` failed: {message}")]
    ToolExecution { tool: String, message: String },
    #[error("maximum tool iterations reached: {0}")]
    MaxIterations(usize),
}
