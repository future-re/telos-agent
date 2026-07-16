use serde_json::Value;
use url::Url;

use crate::error::AgentError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct DomainFilters {
    pub(super) allowed_domains: Vec<String>,
    pub(super) blocked_domains: Vec<String>,
}

impl DomainFilters {
    pub(super) fn from_args(args: &Value) -> Result<Self, AgentError> {
        Ok(Self {
            allowed_domains: parse_domain_list(args, "allowed_domains")?,
            blocked_domains: parse_domain_list(args, "blocked_domains")?,
        })
    }

    pub(super) fn allows(&self, url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        let Some(host) = parsed.host_str() else {
            return false;
        };
        if domain_matches_any(host, &self.blocked_domains) {
            return false;
        }
        self.allowed_domains.is_empty() || domain_matches_any(host, &self.allowed_domains)
    }
}

fn parse_domain_list(args: &Value, key: &str) -> Result<Vec<String>, AgentError> {
    let Some(value) = args.get(key) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
    };
    let mut domains = Vec::new();
    for value in values {
        let Some(domain) = value.as_str() else {
            return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
        };
        let domain = domain.trim().trim_start_matches('.').to_ascii_lowercase();
        if !domain.is_empty() {
            domains.push(domain);
        }
    }
    Ok(domains)
}

fn domain_matches_any(host: &str, domains: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    domains.iter().any(|domain| {
        let domain = domain.trim_start_matches('.').to_ascii_lowercase();
        host == domain || host.ends_with(&format!(".{domain}"))
    })
}

pub(super) fn filter_results(results: &mut Vec<Value>, filters: &DomainFilters) {
    if filters.allowed_domains.is_empty() && filters.blocked_domains.is_empty() {
        return;
    }
    results.retain(|result| {
        result.get("url").and_then(Value::as_str).map(|url| filters.allows(url)).unwrap_or(false)
    });
}
