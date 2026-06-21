pub(super) fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    if ms < 60_000 {
        return format!("{:.1}s", ms as f64 / 1000.0);
    }
    let secs = ms / 1000;
    format!("{}m{}s", secs / 60, secs % 60)
}

pub(super) fn format_turn_tokens(
    input: u64,
    output: u64,
    total: Option<u64>,
    cache_hit: Option<u64>,
    cache_miss: Option<u64>,
    reasoning: Option<u64>,
) -> String {
    let total = total.unwrap_or(input + output);
    if total == 0 {
        return "tokens n/a".to_string();
    }
    let mut text = format!(
        "tokens ↑{} ↓{} total {}",
        format_token_count(input),
        format_token_count(output),
        format_token_count(total)
    );
    if let Some(tokens) = reasoning {
        text.push_str(&format!(" · reasoning {}", format_token_count(tokens)));
    }
    if cache_hit.is_some() || cache_miss.is_some() {
        text.push_str(&format!(
            " · cache hit {} miss {}",
            cache_hit.map(format_token_count).unwrap_or_else(|| "n/a".to_string()),
            cache_miss.map(format_token_count).unwrap_or_else(|| "n/a".to_string())
        ));
    }
    text
}

fn format_token_count(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    format!("{:.1}k", tokens as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::{format_duration_ms, format_turn_tokens};

    #[test]
    fn formats_duration_tools_and_tokens() {
        assert_eq!(format_duration_ms(850), "850ms");
        assert_eq!(format_duration_ms(12_340), "12.3s");
        assert_eq!(format_duration_ms(65_000), "1m5s");
        assert_eq!(
            format_turn_tokens(12_300, 1_800, None, None, None, None),
            "tokens ↑12.3k ↓1.8k total 14.1k"
        );
        assert_eq!(format_turn_tokens(0, 0, None, None, None, None), "tokens n/a");
        assert_eq!(
            format_turn_tokens(20, 7, Some(27), Some(12), Some(8), Some(4)),
            "tokens ↑20 ↓7 total 27 · reasoning 4 · cache hit 12 miss 8"
        );
    }
}
