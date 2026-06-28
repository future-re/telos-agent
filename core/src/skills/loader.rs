use crate::skills::{Skill, SkillArg, SkillSource};
use std::path::Path;

impl SkillLoader {
    /// Load all skills compiled into the binary.
    pub fn load_bundled_skills() -> Vec<Skill> {
        let mut skills = Vec::new();
        let files: &[(&str, &str)] = &[
            ("verify.md", include_str!("bundled/verify.md")),
            ("debug.md", include_str!("bundled/debug.md")),
            ("remember.md", include_str!("bundled/remember.md")),
            ("brainstorm.md", include_str!("bundled/brainstorm.md")),
            ("update-config.md", include_str!("bundled/update-config.md")),
            ("explore.md", include_str!("bundled/explore.md")),
            ("web-access.md", include_str!("bundled/web-access.md")),
            ("powershell-use.md", include_str!("bundled/powershell-use.md")),
        ];
        for (_name, content) in files {
            if let Some(skill) = Self::parse_skill(content, SkillSource::Bundled) {
                skills.push(skill);
            }
        }
        skills
    }
}

/// Loads skills from a directory of markdown files.
pub struct SkillLoader;

impl SkillLoader {
    /// Load a single skill from a markdown file with YAML frontmatter.
    pub fn load_skill_file(path: &Path, source: SkillSource) -> Option<Skill> {
        let content = std::fs::read_to_string(path).ok()?;
        Self::parse_skill(&content, source)
    }

    /// Scan `dir` for `.md` files, parse YAML frontmatter, return skills.
    /// Non-fatal errors (unparseable files) are logged and skipped.
    pub fn load_from_dir(dir: &Path) -> Result<Vec<Skill>, std::io::Error> {
        let mut skills = Vec::new();
        if !dir.exists() {
            return Ok(skills);
        }
        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Some(skill) = Self::parse_skill(&content, SkillSource::Project) {
                    skills.push(skill);
                } else {
                    tracing::warn!("failed to parse skill file: {}", path.display());
                }
            }
        }
        Ok(skills)
    }

    /// Parse a single skill from markdown content with YAML frontmatter.
    /// Frontmatter is delimited by `---` at the start and end.
    fn parse_skill(content: &str, source: SkillSource) -> Option<Skill> {
        let content = content.trim();
        // Must start with "---"
        let rest = content.strip_prefix("---")?;
        // Find closing "---"
        let (frontmatter, body) = rest.split_once("\n---")?;
        let body = body.trim().to_string();

        let fm: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;

        let name = fm.get("name")?.as_str()?.to_string();
        let description = fm.get("description")?.as_str()?.to_string();
        let when_to_use = fm.get("whenToUse").and_then(|v| v.as_str()).map(String::from);
        let prompt = fm.get("prompt")?.as_str()?.to_string();

        let arguments = fm
            .get("arguments")
            .and_then(|v| v.as_sequence())
            .map(|args| {
                args.iter()
                    .map(|a| SkillArg {
                        name: a.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        description: a
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        required: a.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Some(Skill { name, description, when_to_use, prompt, arguments, body, source })
    }
}
