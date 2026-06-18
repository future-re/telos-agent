//! PluginRegistry — manages loaded plugins and their enable/disable lifecycle.

pub use lifecycle::PluginRegistry;
pub use types::{LoadedPlugin, PluginEntry, PluginStatus};

mod apply;
mod discovery;
mod lifecycle;
mod persistence;
#[cfg(test)]
mod tests;
mod types;
