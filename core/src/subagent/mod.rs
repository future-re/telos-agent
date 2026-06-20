//! Subagent module — in-process nested agents and the Fork concurrent-execution engine.

pub mod builtins;
pub mod definition;
pub mod fork;
pub mod registry;
mod tool;

pub use definition::{AgentDefinition, AgentIsolation, AgentSource};
pub use fork::{ForkExecution, ForkLens, ForkResult, ForkShared, Synapse};
pub use registry::SubagentRegistry;
pub use tool::SubagentTool;
