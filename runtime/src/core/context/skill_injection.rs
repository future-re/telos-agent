use std::sync::Arc;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::message::SystemReminder;
use crate::skills::SkillRegistry;

pub struct SkillInjector {
    registry: Arc<SkillRegistry>,
    max_skills: usize,
}

pub struct SkillInjection {
    pub reminder: SystemReminder,
    pub fingerprint: u64,
}

impl SkillInjector {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry, max_skills: 3 }
    }

    pub fn with_max_skills(mut self, max: usize) -> Self {
        self.max_skills = max;
        self
    }

    pub fn inject_for_query(&self, query: &str) -> Option<SkillInjection> {
        let skills = self.registry.retrieve(query, self.max_skills);
        if skills.is_empty() {
            return None;
        }

        let mut lines = vec![
            "## Recommended Skills".to_string(),
            "Use the Skill tool only with the skills listed below; do not guess skill names."
                .to_string(),
            String::new(),
        ];
        for skill in skills {
            let when = skill
                .when_to_use
                .as_ref()
                .map(|when| format!(" Use when: {when}"))
                .unwrap_or_default();
            lines.push(format!("- **{}**: {}{}", skill.name, skill.description, when));
        }

        let content = lines.join("\n");
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        Some(SkillInjection {
            reminder: SystemReminder::SkillDiscovery { content },
            fingerprint: hasher.finish(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{Skill, SkillRegistry, SkillSource};

    fn test_skill(name: &str, description: &str, body: &str) -> Skill {
        Skill {
            name: name.into(),
            description: description.into(),
            when_to_use: Some("When the task matches".into()),
            prompt: "Prompt".into(),
            arguments: vec![],
            body: body.into(),
            source: SkillSource::Bundled,
        }
    }

    #[test]
    fn inject_for_query_returns_relevant_skills() {
        let mut registry = SkillRegistry::new();
        registry.register(test_skill("rust-fix", "Fix Rust compiler errors", "cargo check"));
        registry.register(test_skill("react-ui", "Adjust React UI layout", "jsx css layout"));
        let injector = SkillInjector::new(Arc::new(registry));

        let injection = injector.inject_for_query("fix rust compile error").expect("injection");
        let rendered = injection.reminder.render();
        assert!(rendered.contains("Recommended Skills"));
        assert!(rendered.contains("rust-fix"));
        assert!(!rendered.contains("react-ui"));
    }
}
