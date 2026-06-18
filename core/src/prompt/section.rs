use async_trait::async_trait;

/// A rendered prompt section with caching metadata.
#[derive(Debug, Clone)]
pub struct PromptBlock {
    pub name: String,
    pub text: String,
    pub stability: PromptStability,
}

/// Hint to providers about whether a block should be cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHint {
    Static,
    Dynamic,
}

impl From<PromptStability> for CacheHint {
    fn from(value: PromptStability) -> Self {
        match value {
            PromptStability::Static => CacheHint::Static,
            PromptStability::Dynamic => CacheHint::Dynamic,
        }
    }
}

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

#[async_trait]
impl PromptSection for Box<dyn PromptSection> {
    fn name(&self) -> &str {
        self.as_ref().name()
    }
    fn stability(&self) -> PromptStability {
        self.as_ref().stability()
    }
    async fn render(&self, ctx: &()) -> String {
        self.as_ref().render(ctx).await
    }
}
