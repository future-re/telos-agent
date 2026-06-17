//! Prompt system — modular, cache-aware construction of the system prompt.
pub mod assembly;
pub mod section;
pub use assembly::PromptAssembly;
pub use section::{PromptSection, PromptStability};
