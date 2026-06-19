//! CodeQL integration — static analysis via GitHub's CodeQL CLI.
//!
//! This module provides:
//! - [`CodeqlConfig`] — runtime configuration (enabled, query packs, timeouts).
//! - [`SarifParser`] — parses SARIF 2.1 JSON output from CodeQL.
//! - [`CodeqlDatabase`] — manages CodeQL database lifecycle (create, update, detect).
//! - [`CodeQLTool`] — agent-callable tool that runs CodeQL queries and stores findings.
//! - [`CodeqlSection`] — prompt section that injects active CodeQL findings into the
//!   system prompt.
//!
//! Everything is gated at runtime via [`CodeqlConfig::enabled`] and the availability
//! of the `codeql` CLI on `PATH`.  When the CLI is missing or the config is disabled,
//! the tool returns a clear error and the runtime skips startup analysis.

pub mod config;
pub mod database;
pub mod sarif;
pub mod section;
pub mod tool;

pub use config::CodeqlConfig;
pub use database::CodeqlDatabase;
pub use sarif::{SarifParser, SarifResult};
pub use section::CodeqlSection;
pub use tool::CodeQLTool;
