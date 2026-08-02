//! Source types for marketplaces — where marketplace.json comes from.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where a marketplace manifest is fetched from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
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
    /// Local directory containing marketplace.json.
    #[serde(rename_all = "camelCase")]
    Local { path: PathBuf },
}
