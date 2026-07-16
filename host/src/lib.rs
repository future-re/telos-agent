//! Shared host-layer assembly for Telos CLI and desktop applications.

pub use telos_agent::*;

pub mod config;
pub mod context;
pub mod memory_runtime;
pub mod options;
pub mod project;
pub mod runtime;

pub use options::{ProviderKind, ProviderSetup, SharedOptions};
pub use project::find_project_root;
