//! Source types for marketplaces — where marketplace.json comes from.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::integrations::plugin::manifest::MarketplaceEntry;

/// Where a marketplace manifest is fetched from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MarketplaceSource {
    /// GitHub repository containing marketplace.json.
    #[serde(rename_all = "camelCase")]
    GitHub {
        repo: String,
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Arbitrary git URL.
    #[serde(rename_all = "camelCase")]
    Git {
        url: String,
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Direct URL to marketplace.json.
    #[serde(rename_all = "camelCase")]
    Url { url: String },
    /// npm package containing marketplace.json.
    #[serde(rename_all = "camelCase")]
    Npm { package: String },
    /// Local directory containing marketplace.json.
    #[serde(rename_all = "camelCase")]
    Local { path: PathBuf },
    /// Inline marketplace defined in config (no remote fetch needed).
    #[serde(rename_all = "camelCase")]
    Inline { name: String, plugins: Vec<MarketplaceEntry> },
}
