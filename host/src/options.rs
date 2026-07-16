use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Deepseek,
    Mock,
}

#[derive(Debug, Clone, Default)]
pub struct SharedOptions {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub thinking_model: Option<String>,
    pub fast_model: Option<String>,
    pub api_key: Option<String>,
    pub cwd: Option<PathBuf>,
    pub max_iterations: Option<usize>,
    pub no_validate_schema: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderSetup {
    pub provider: ProviderKind,
    pub api_key: String,
    pub thinking_model: String,
    pub fast_model: String,
}
