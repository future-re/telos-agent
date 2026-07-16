//! Skills system — user-defined slash-commands loaded from markdown files.
//!
//! Skills are Markdown files with YAML frontmatter. They are loaded from
//! directories in priority order and injected into the system prompt.

pub mod loader;
pub mod registry;

pub use loader::SkillLoader;
pub use registry::SkillRegistry;

/// A loaded skill ready for invocation.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub prompt: String,
    pub arguments: Vec<SkillArg>,
    pub body: String,
    pub source: SkillSource,
}

/// Description of a skill argument for template substitution.
#[derive(Debug, Clone)]
pub struct SkillArg {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// Where a skill was loaded from — determines override priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    Bundled,
    Managed,
    Project,
    User,
    /// Loaded from an installed plugin.
    Plugin {
        plugin_id: crate::integrations::plugin::PluginId,
    },
}
