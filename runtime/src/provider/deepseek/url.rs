pub(super) fn normalize_chat_url(base_url: &str) -> String {
    normalize_v1_url(base_url, "chat/completions")
}

pub(super) fn normalize_v1_url(base_url: &str, path: &str) -> String {
    normalize_url(base_url, "v1", path)
}

pub(super) fn normalize_beta_url(base_url: &str, path: &str) -> String {
    normalize_url(base_url, "beta", path)
}

fn normalize_url(base_url: &str, api_prefix: &str, path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let path = path.trim_matches('/');
    let full_suffix = format!("/{api_prefix}/{path}");
    if trimmed.ends_with(&full_suffix) {
        trimmed.to_string()
    } else if let Some(base) = trimmed.strip_suffix("/v1").or_else(|| trimmed.strip_suffix("/beta"))
    {
        format!("{base}/{api_prefix}/{path}")
    } else {
        format!("{trimmed}/{api_prefix}/{path}")
    }
}
