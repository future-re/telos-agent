use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStability {
    Static,
    Dynamic,
}

#[async_trait]
pub trait PromptSection: Send + Sync {
    fn name(&self) -> &str;
    fn stability(&self) -> PromptStability;
    async fn render(&self, _ctx: &()) -> String;
}
