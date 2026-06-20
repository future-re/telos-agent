use serde_json::Value;

use crate::error::AgentError;

pub(crate) fn parse_domain_list(args: &Value, key: &str) -> Result<Vec<String>, AgentError> {
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

pub(crate) fn domain_matches_any(host: &str, domains: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    domains.iter().any(|domain| {
        let domain = domain.trim_start_matches('.').to_ascii_lowercase();
        host == domain || host.ends_with(&format!(".{domain}"))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{domain_matches_any, parse_domain_list};

    #[test]
    fn domain_list_normalizes_case_dots_and_empty_values() {
        let args = json!({ "allowed_domains": [" Example.COM ", ".docs.rs", "", "  "] });

        assert_eq!(
            parse_domain_list(&args, "allowed_domains").unwrap(),
            vec!["example.com".to_string(), "docs.rs".to_string()]
        );
    }

    #[test]
    fn domain_matching_accepts_exact_hosts_and_subdomains() {
        let domains = vec!["example.com".to_string()];

        assert!(domain_matches_any("example.com", &domains));
        assert!(domain_matches_any("docs.example.com", &domains));
        assert!(!domain_matches_any("badexample.com", &domains));
    }

    #[test]
    fn parse_domain_list_rejects_non_string_domain_entries() {
        let args = json!({ "blocked_domains": ["example.com", 3] });

        assert!(parse_domain_list(&args, "blocked_domains").is_err());
    }
}
