use std::collections::HashMap;
use std::path::Path;

use crate::skills::loader::SkillLoader;
use crate::skills::{Skill, SkillSource};

/// Name-indexed collection of skills with override priority.
///
/// Later registrations with the same name override earlier ones,
/// implementing the priority chain: Bundled < Managed < Project < User.
#[derive(Debug, Default, Clone)]
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a skill. Same name → later wins.
    pub fn register(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    /// Load all .md files from dir and register them with the given source.
    pub fn inject_skills_from_dir(
        &mut self,
        dir: &Path,
        source: SkillSource,
    ) -> std::io::Result<()> {
        let skills = SkillLoader::load_from_dir(dir)?;
        for mut skill in skills {
            skill.source = source.clone();
            self.register(skill);
        }
        Ok(())
    }

    /// Look up a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// All registered skill names.
    pub fn names(&self) -> Vec<&String> {
        self.skills.keys().collect()
    }

    /// All registered skills.
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Render the skills list for injection into the system prompt.
    pub fn render_for_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## Available Skills".to_string()];
        lines.push("You can invoke skills via the Skill tool. Available skills:".to_string());
        for skill in self.skills.values() {
            let when =
                skill.when_to_use.as_ref().map(|w| format!(" — Use when: {w}")).unwrap_or_default();
            lines.push(format!("- **{}**: {}{}", skill.name, skill.description, when));
        }
        lines.join("\n")
    }
}
