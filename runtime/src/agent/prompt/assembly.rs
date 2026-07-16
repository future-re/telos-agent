use crate::agent::prompt::PromptSection;
use crate::agent::prompt::section::{PromptBlock, PromptStability};
use std::collections::HashMap;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSectionStat {
    pub name: String,
    pub chars: usize,
    pub stability: PromptStability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptStats {
    pub total_chars: usize,
    pub sections: Vec<PromptSectionStat>,
}

/// Assembles a system prompt from ordered sections with caching.
/// Static sections are rendered once and cached; dynamic re-render each time.
pub struct PromptAssembly {
    sections: Vec<Box<dyn PromptSection>>,
    static_cache: Mutex<HashMap<String, String>>,
}

impl Default for PromptAssembly {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptAssembly {
    pub fn new() -> Self {
        Self { sections: Vec::new(), static_cache: Mutex::new(HashMap::new()) }
    }

    /// Add a section whose stability is determined by its own
    /// [`PromptSection::stability`] implementation.
    pub fn add(&mut self, section: impl PromptSection + 'static) {
        self.sections.push(Box::new(section));
    }

    pub async fn build(&self) -> String {
        let mut parts = Vec::new();
        for section in &self.sections {
            let text = self.render_section(section).await;
            if !text.is_empty() {
                parts.push(text);
            }
        }
        parts.join("\n\n")
    }

    /// Render the assembly into structured blocks.
    pub async fn build_blocks(&self) -> Vec<PromptBlock> {
        let mut blocks = Vec::new();
        for section in &self.sections {
            let text = self.render_section(section).await;
            if !text.is_empty() {
                blocks.push(PromptBlock {
                    name: section.name().to_string(),
                    text,
                    stability: section.stability(),
                });
            }
        }
        blocks
    }

    pub async fn build_stats(&self) -> PromptStats {
        let blocks = self.build_blocks().await;
        let sections = blocks
            .iter()
            .map(|block| PromptSectionStat {
                name: block.name.clone(),
                chars: block.text.chars().count(),
                stability: block.stability,
            })
            .collect::<Vec<_>>();
        let total_chars = sections.iter().map(|section| section.chars).sum();
        PromptStats { total_chars, sections }
    }

    async fn render_section(&self, section: &dyn PromptSection) -> String {
        match section.stability() {
            PromptStability::Static => {
                let mut cache = self.static_cache.lock().await;
                if let Some(cached) = cache.get(section.name()) {
                    cached.clone()
                } else {
                    let rendered = section.render(&()).await;
                    cache.insert(section.name().to_string(), rendered.clone());
                    rendered
                }
            }
            PromptStability::Dynamic => section.render(&()).await,
        }
    }
}
