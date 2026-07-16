use std::collections::HashMap;
use std::path::Path;

use crate::knowledge::skills::loader::SkillLoader;
use crate::knowledge::skills::{Skill, SkillSource};

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

    /// Load all skills bundled with the crate and register them.
    pub fn load_bundled_skills(&mut self) {
        for skill in SkillLoader::load_bundled_skills() {
            self.register(skill);
        }
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

    /// Retrieve the most relevant skills for a free-text query.
    pub fn retrieve(&self, query: &str, limit: usize) -> Vec<&Skill> {
        if limit == 0 {
            return Vec::new();
        }

        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let mut ranked = self
            .skills
            .values()
            .filter_map(|skill| {
                let score = score_skill(skill, &query_terms);
                (score > 0).then_some((score, skill))
            })
            .collect::<Vec<_>>();

        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        ranked.into_iter().take(limit).map(|(_, skill)| skill).collect()
    }

    /// Render a compact skill index for lightweight prompt injection.
    pub fn render_index_for_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut lines = vec![
            "## Skill Index".to_string(),
            "Use the Skill tool only for skills listed as available or recommended below."
                .to_string(),
        ];
        let mut skills: Vec<&Skill> = self.skills.values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        for skill in skills {
            lines.push(format!("- **{}**: {}", skill.name, skill.description));
        }
        lines.join("\n")
    }

    /// Render the skills list for injection into the system prompt.
    pub fn render_for_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## Available Skills".to_string()];
        lines.push("You can invoke skills via the Skill tool. Available skills:".to_string());
        let mut skills: Vec<&Skill> = self.skills.values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        for skill in skills {
            let when =
                skill.when_to_use.as_ref().map(|w| format!(" — Use when: {w}")).unwrap_or_default();
            lines.push(format!("- **{}**: {}{}", skill.name, skill.description, when));
        }
        lines.join("\n")
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn score_skill(skill: &Skill, query_terms: &[String]) -> usize {
    let name = skill.name.to_ascii_lowercase();
    let description = skill.description.to_ascii_lowercase();
    let when = skill.when_to_use.as_deref().unwrap_or("").to_ascii_lowercase();
    let body = skill.body.to_ascii_lowercase();
    let prompt = skill.prompt.to_ascii_lowercase();

    query_terms.iter().fold(0usize, |score, term| {
        score
            + if name.contains(term) { 6 } else { 0 }
            + if description.contains(term) { 4 } else { 0 }
            + if when.contains(term) { 3 } else { 0 }
            + if prompt.contains(term) { 2 } else { 0 }
            + if body.contains(term) { 1 } else { 0 }
    })
}
