use crate::prompt::{PromptSection, PromptStability};
use std::collections::HashMap;
use tokio::sync::Mutex;

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

    pub fn add_section(&mut self, section: Box<dyn PromptSection>) {
        self.sections.push(section);
    }

    pub fn add_static(&mut self, section: impl PromptSection + 'static) {
        self.add_section(Box::new(section));
    }

    pub fn add_dynamic(&mut self, section: impl PromptSection + 'static) {
        self.add_section(Box::new(section));
    }

    pub async fn build(&self) -> String {
        let mut parts = Vec::new();
        for section in &self.sections {
            let text = match section.stability() {
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
            };
            if !text.is_empty() {
                parts.push(text);
            }
        }
        parts.join("\n\n")
    }
}
