//! Cost estimation for LLM usage.
//!
//! The calculator uses model prices from `[billing.models]` in the telos config
//! and falls back to built-in defaults for known DeepSeek V4 models.

use std::collections::HashMap;

use telos_agent::TokenUsage;

use crate::config::{BillingModelPricing, BillingSection};

/// Currency-agnostic cost value. Prices are stored per-million tokens and
/// returned here as the same unit the caller configured (e.g. yuan or USD).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CostEstimate {
    pub input_cache_hit: f64,
    pub input_cache_miss: f64,
    pub output: f64,
    pub total: f64,
}

impl CostEstimate {
    /// Sum the individual buckets into `total`.
    pub fn compute_total(&mut self) {
        self.total = self.input_cache_hit + self.input_cache_miss + self.output;
    }
}

/// Calculates token costs from a billing configuration.
#[derive(Debug, Clone, Default)]
pub struct CostCalculator {
    models: HashMap<String, BillingModelPricing>,
}

/// Known billing-model key prefixes, ordered longest-first for correct
/// prefix matching against API model names (e.g. "deepseek-v4-flash-20250301").
const KNOWN_MODEL_KEY_PREFIXES: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];

/// Common DeepSeek API model name aliases → billing key mappings.
/// Used when prefix matching also fails (e.g. "deepseek-chat" → flash).
const MODEL_NAME_ALIASES: &[(&str, &str)] = &[("deepseek-chat", "deepseek-v4-flash")];

impl CostCalculator {
    /// DeepSeek V4-Flash defaults (yuan per million tokens).
    ///
    /// These match the published price card:
    /// - input cache hit: 0.02
    /// - input cache miss: 1.0
    /// - output: 2.0
    pub const DEFAULT_FLASH_HIT: f64 = 0.02;
    pub const DEFAULT_FLASH_MISS: f64 = 1.0;
    pub const DEFAULT_FLASH_OUTPUT: f64 = 2.0;

    /// DeepSeek V4-Pro defaults (yuan per million tokens, promo-era estimate).
    ///
    /// V4-Pro is roughly 3.1x more expensive than Flash on cache-miss input and
    /// output, and ~1.3x on cache-hit input. Users should override these via
    /// `[billing.models.deepseek-v4-pro]` if their account uses list pricing.
    pub const DEFAULT_PRO_HIT: f64 = 0.0261;
    pub const DEFAULT_PRO_MISS: f64 = 3.132;
    pub const DEFAULT_PRO_OUTPUT: f64 = 6.264;

    /// Build a calculator from a `[billing]` config section.
    ///
    /// Prices in `billing` override the built-in defaults; missing prices fall
    /// back to the defaults for `deepseek-v4-flash` and `deepseek-v4-pro`.
    pub fn from_section(section: Option<&BillingSection>) -> Self {
        let mut models = HashMap::new();

        // Built-in defaults for the two known DeepSeek V4 tiers.
        models.insert(
            "deepseek-v4-flash".into(),
            BillingModelPricing {
                input_cache_hit_per_million: Some(Self::DEFAULT_FLASH_HIT),
                input_cache_miss_per_million: Some(Self::DEFAULT_FLASH_MISS),
                output_per_million: Some(Self::DEFAULT_FLASH_OUTPUT),
            },
        );
        models.insert(
            "deepseek-v4-pro".into(),
            BillingModelPricing {
                input_cache_hit_per_million: Some(Self::DEFAULT_PRO_HIT),
                input_cache_miss_per_million: Some(Self::DEFAULT_PRO_MISS),
                output_per_million: Some(Self::DEFAULT_PRO_OUTPUT),
            },
        );

        if let Some(section) = section
            && let Some(user_models) = &section.models
        {
            for (name, pricing) in user_models {
                let existing = models.get(name).cloned().unwrap_or_default();
                models.insert(
                    name.clone(),
                    BillingModelPricing {
                        input_cache_hit_per_million: pricing
                            .input_cache_hit_per_million
                            .or(existing.input_cache_hit_per_million),
                        input_cache_miss_per_million: pricing
                            .input_cache_miss_per_million
                            .or(existing.input_cache_miss_per_million),
                        output_per_million: pricing
                            .output_per_million
                            .or(existing.output_per_million),
                    },
                );
            }
        }

        Self { models }
    }

    /// Resolve an API-reported model name to a billing config key.
    ///
    /// Resolution order:
    /// 1. Exact match against billing config keys
    /// 2. Prefix match — e.g. "deepseek-v4-flash-20250301" → "deepseek-v4-flash"
    /// 3. Alias lookup — e.g. "deepseek-chat" → "deepseek-v4-flash"
    /// 4. Returns `None` if nothing matches
    fn resolve_model_key<'a>(&'a self, api_model: &'a str) -> Option<&'a str> {
        // 1. Exact match
        if self.models.contains_key(api_model) {
            return Some(api_model);
        }
        // 2. Prefix match (longest-first ordering ensures correct match)
        for key in KNOWN_MODEL_KEY_PREFIXES {
            if api_model.starts_with(key) {
                return Some(key);
            }
        }
        // 3. Alias lookup
        for &(alias, canonical) in MODEL_NAME_ALIASES {
            if api_model.eq_ignore_ascii_case(alias) {
                return Some(canonical);
            }
        }
        None
    }

    /// Estimate cost for a single provider response.
    ///
    /// If the model has no configured prices, returns `None`. When cache
    /// hit/miss breakdowns are missing, all input tokens are billed at the
    /// cache-miss rate.
    pub fn estimate(&self, model: Option<&str>, usage: &TokenUsage) -> Option<CostEstimate> {
        let model = model?;
        let key = self.resolve_model_key(model)?;
        let pricing = self.models.get(key)?;

        let hit_rate = pricing.input_cache_hit_per_million?;
        let miss_rate = pricing.input_cache_miss_per_million?;
        let output_rate = pricing.output_per_million?;

        let (hit_tokens, miss_tokens) =
            match (usage.prompt_cache_hit_tokens, usage.prompt_cache_miss_tokens) {
                (Some(hit), Some(miss)) => (hit as f64, miss as f64),
                (Some(hit), None) => {
                    // Provider reported only cache hits; treat the remainder of the
                    // input as cache misses.
                    let miss = usage.input_tokens.saturating_sub(hit);
                    (hit as f64, miss as f64)
                }
                (None, Some(miss)) => {
                    let hit = usage.input_tokens.saturating_sub(miss);
                    (hit as f64, miss as f64)
                }
                (None, None) => (0.0, usage.input_tokens as f64),
            };

        let mut estimate = CostEstimate {
            input_cache_hit: hit_tokens * hit_rate / 1_000_000.0,
            input_cache_miss: miss_tokens * miss_rate / 1_000_000.0,
            output: usage.output_tokens as f64 * output_rate / 1_000_000.0,
            total: 0.0,
        };
        estimate.compute_total();
        Some(estimate)
    }

    /// Format a cost as a compact human-readable string.
    ///
    /// Examples: "¥0.12", "¥1.2k" for very large values.
    pub fn format_cost(cost: f64) -> String {
        if cost >= 1_000_000.0 {
            format!("¥{:.1}m", cost / 1_000_000.0)
        } else if cost >= 1_000.0 {
            format!("¥{:.1}k", cost / 1_000.0)
        } else if cost >= 1.0 {
            format!("¥{:.2}", cost)
        } else if cost >= 0.01 {
            format!("¥{:.3}", cost)
        } else if cost > 0.0 {
            format!("¥{:.4}", cost)
        } else {
            "¥0".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_usage(
        input: usize,
        output: usize,
        hit: Option<usize>,
        miss: Option<usize>,
    ) -> TokenUsage {
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: Some(input + output),
            prompt_cache_hit_tokens: hit,
            prompt_cache_miss_tokens: miss,
            reasoning_tokens: None,
        }
    }

    #[test]
    fn flash_pricing_with_cache_breakdown() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(1_000_000, 500_000, Some(800_000), Some(200_000));
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert!((est.input_cache_hit - 0.016).abs() < 0.0001, "hit cost: {}", est.input_cache_hit);
        assert!((est.input_cache_miss - 0.2).abs() < 0.0001, "miss cost: {}", est.input_cache_miss);
        assert!((est.output - 1.0).abs() < 0.0001, "output cost: {}", est.output);
        assert!((est.total - 1.216).abs() < 0.0001, "total cost: {}", est.total);
    }

    #[test]
    fn falls_back_to_cache_miss_when_no_cache_breakdown() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(1_000_000, 500_000, None, None);
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert_eq!(est.input_cache_hit, 0.0);
        assert!((est.input_cache_miss - 1.0).abs() < 0.0001);
        assert!((est.total - 2.0).abs() < 0.0001);
    }

    #[test]
    fn unknown_model_returns_none() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(100, 100, None, None);
        assert!(calc.estimate(Some("unknown-model"), &usage).is_none());
    }

    #[test]
    fn user_config_overrides_defaults() {
        let section = BillingSection {
            models: Some(HashMap::from([(
                "deepseek-v4-flash".into(),
                BillingModelPricing {
                    input_cache_hit_per_million: Some(0.01),
                    input_cache_miss_per_million: Some(0.5),
                    output_per_million: Some(1.0),
                },
            )])),
        };
        let calc = CostCalculator::from_section(Some(&section));
        let usage = make_usage(1_000_000, 500_000, Some(500_000), Some(500_000));
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert!((est.input_cache_hit - 0.005).abs() < 0.0001);
        assert!((est.input_cache_miss - 0.25).abs() < 0.0001);
        assert!((est.output - 0.5).abs() < 0.0001);
    }

    #[test]
    fn formats_cost_values() {
        assert_eq!(CostCalculator::format_cost(0.0), "¥0");
        assert_eq!(CostCalculator::format_cost(0.00123), "¥0.0012");
        assert_eq!(CostCalculator::format_cost(0.1234), "¥0.123");
        assert_eq!(CostCalculator::format_cost(1.234), "¥1.23");
        assert_eq!(CostCalculator::format_cost(1234.0), "¥1.2k");
        assert_eq!(CostCalculator::format_cost(1_234_567.0), "¥1.2m");
    }

    #[test]
    fn pro_pricing_with_cache_breakdown() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(1_000_000, 500_000, Some(800_000), Some(200_000));
        let est = calc.estimate(Some("deepseek-v4-pro"), &usage).unwrap();

        assert!(
            (est.input_cache_hit - 0.02088).abs() < 0.0001,
            "hit cost: {}",
            est.input_cache_hit
        );
        assert!(
            (est.input_cache_miss - 0.6264).abs() < 0.0001,
            "miss cost: {}",
            est.input_cache_miss
        );
        assert!((est.output - 3.132).abs() < 0.0001, "output cost: {}", est.output);
        assert!((est.total - 3.77928).abs() < 0.0001, "total cost: {}", est.total);
    }

    #[test]
    fn pro_is_more_expensive_than_flash() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(1_000_000, 500_000, Some(800_000), Some(200_000));

        let flash = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();
        let pro = calc.estimate(Some("deepseek-v4-pro"), &usage).unwrap();

        assert!(
            pro.total > flash.total * 3.0,
            "pro ({}) should be >3x flash ({})",
            pro.total,
            flash.total
        );
    }

    /// Simulates a full turn with routed provider: one pro call (thinking) +
    /// multiple flash calls (execution tool loops), verifying costs accumulate
    /// correctly per-model.
    #[test]
    fn multi_model_turn_cost_accumulation() {
        let calc = CostCalculator::from_section(None);

        // Turn: 1 pro thinking call → 2 flash execution calls
        let pro_usage = make_usage(2_000_000, 300_000, Some(1_500_000), Some(500_000));
        let flash_call1 = make_usage(100_000, 200_000, Some(80_000), Some(20_000));
        let flash_call2 = make_usage(50_000, 150_000, Some(40_000), Some(10_000));

        let pro_cost = calc.estimate(Some("deepseek-v4-pro"), &pro_usage).unwrap();
        let flash1 = calc.estimate(Some("deepseek-v4-flash"), &flash_call1).unwrap();
        let flash2 = calc.estimate(Some("deepseek-v4-flash"), &flash_call2).unwrap();

        // Verify each is priced at the correct tier
        assert!((pro_cost.total - (3.132 * 0.5 + 6.264 * 0.3 + 0.0261 * 1.5)).abs() < 0.001);
        assert!((flash1.total - (0.02 * 0.08 + 1.0 * 0.02 + 2.0 * 0.2)).abs() < 0.001);
        assert!((flash2.total - (0.02 * 0.04 + 1.0 * 0.01 + 2.0 * 0.15)).abs() < 0.001);

        // Total should be dominated by the single pro call
        assert!(
            pro_cost.total > flash1.total + flash2.total,
            "single pro cost ({}) should exceed both flash calls combined ({})",
            pro_cost.total,
            flash1.total + flash2.total
        );
    }

    /// When the DeepSeek API returns a model name that doesn't exactly match
    /// the billing config keys, prefix matching and aliases should still resolve
    /// to the correct pricing.
    #[test]
    fn api_model_name_aliases_resolve_to_pricing() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(1000, 500, Some(500), Some(500));

        // Prefix matching: model name with version suffix
        let est = calc.estimate(Some("deepseek-v4-flash-20250301"), &usage).unwrap();
        assert!(
            (est.total - 0.00151).abs() < 0.0001,
            "flash with version suffix should resolve: {est:?}"
        );

        let est = calc.estimate(Some("deepseek-v4-pro-beta"), &usage).unwrap();
        assert!(
            (est.total - 0.004711).abs() < 0.0001,
            "pro with version suffix should resolve: {est:?}"
        );

        // Alias lookup: historical model name
        let est = calc.estimate(Some("deepseek-chat"), &usage).unwrap();
        assert!(
            (est.total - 0.00151).abs() < 0.0001,
            "deepseek-chat alias should resolve to flash: {est:?}"
        );

        // Completely unknown model still returns None
        assert!(calc.estimate(Some("gpt-4"), &usage).is_none());
    }

    #[test]
    fn model_is_none_returns_none() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(1000, 500, None, None);
        assert!(calc.estimate(None, &usage).is_none());
    }

    #[test]
    fn zero_tokens_returns_zero_cost() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(0, 0, Some(0), Some(0));
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert_eq!(est.input_cache_hit, 0.0);
        assert_eq!(est.input_cache_miss, 0.0);
        assert_eq!(est.output, 0.0);
        assert_eq!(est.total, 0.0);
    }

    #[test]
    fn all_cache_hit_no_miss() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(1_000_000, 100_000, Some(1_000_000), Some(0));
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert!((est.input_cache_hit - 0.02).abs() < 0.0001);
        assert_eq!(est.input_cache_miss, 0.0);
        assert!((est.total - 0.22).abs() < 0.0001);
    }

    #[test]
    fn partial_cache_breakdown_hit_only() {
        let calc = CostCalculator::from_section(None);
        // Only hit reported; miss inferred: 1M input - 800k hit = 200k miss
        let usage = make_usage(1_000_000, 500_000, Some(800_000), None);
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert!((est.input_cache_hit - 0.016).abs() < 0.0001);
        assert!(
            (est.input_cache_miss - 0.2).abs() < 0.0001,
            "miss should be inferred, got {}",
            est.input_cache_miss
        );
    }

    #[test]
    fn partial_cache_breakdown_miss_only() {
        let calc = CostCalculator::from_section(None);
        // Only miss reported; hit inferred: 1M input - 200k miss = 800k hit
        let usage = make_usage(1_000_000, 500_000, None, Some(200_000));
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert!(
            (est.input_cache_hit - 0.016).abs() < 0.0001,
            "hit should be inferred, got {}",
            est.input_cache_hit
        );
        assert!((est.input_cache_miss - 0.2).abs() < 0.0001);
    }

    #[test]
    fn user_config_partial_override_keeps_defaults() {
        let section = BillingSection {
            models: Some(HashMap::from([(
                "deepseek-v4-flash".into(),
                BillingModelPricing {
                    input_cache_hit_per_million: None,
                    input_cache_miss_per_million: None,
                    output_per_million: Some(1.0), // override only output
                },
            )])),
        };
        let calc = CostCalculator::from_section(Some(&section));
        let usage = make_usage(1_000_000, 500_000, Some(800_000), Some(200_000));
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        // Hit/miss use defaults; output uses overridden value
        assert!((est.input_cache_hit - 0.016).abs() < 0.0001, "hit: {}", est.input_cache_hit);
        assert!((est.input_cache_miss - 0.2).abs() < 0.0001, "miss: {}", est.input_cache_miss);
        assert!((est.output - 0.5).abs() < 0.0001, "output: {}", est.output);
    }

    #[test]
    fn custom_model_without_defaults_returns_none() {
        let calc = CostCalculator::from_section(None);
        let usage = make_usage(100, 100, None, None);
        // Model not in flash/pro defaults and not in user config
        assert!(calc.estimate(Some("custom-model"), &usage).is_none());

        // But if user configures it, it should work
        let section = BillingSection {
            models: Some(HashMap::from([(
                "custom-model".into(),
                BillingModelPricing {
                    input_cache_hit_per_million: Some(0.5),
                    input_cache_miss_per_million: Some(2.0),
                    output_per_million: Some(4.0),
                },
            )])),
        };
        let calc = CostCalculator::from_section(Some(&section));
        assert!(calc.estimate(Some("custom-model"), &usage).is_some());
    }

    #[test]
    fn large_numbers_do_not_overflow() {
        let calc = CostCalculator::from_section(None);
        // 100M tokens each (extremely long context)
        let usage = make_usage(100_000_000, 50_000_000, Some(80_000_000), Some(20_000_000));
        let est = calc.estimate(Some("deepseek-v4-flash"), &usage).unwrap();

        assert!((est.input_cache_hit - 1.6).abs() < 0.01);
        assert!((est.input_cache_miss - 20.0).abs() < 0.01);
        assert!((est.output - 100.0).abs() < 0.01);
        assert!((est.total - 121.6).abs() < 0.01);
        assert!(!est.total.is_nan());
        assert!(!est.total.is_infinite());
    }

    #[test]
    fn fmt_zero_cost() {
        assert_eq!(CostCalculator::format_cost(0.0), "¥0");
    }

    #[test]
    fn fmt_very_small_cost() {
        assert_eq!(CostCalculator::format_cost(0.000_012_3), "¥0.0000");
    }

    #[test]
    fn fmt_cost_rounding_boundaries() {
        assert_eq!(CostCalculator::format_cost(0.00999), "¥0.0100");
        assert_eq!(CostCalculator::format_cost(0.999), "¥0.999");
        assert_eq!(CostCalculator::format_cost(999.0), "¥999.00");
        assert_eq!(CostCalculator::format_cost(999.999), "¥1000.00");
        assert_eq!(CostCalculator::format_cost(9_999.0), "¥10.0k");
        assert_eq!(CostCalculator::format_cost(999_999.0), "¥1000.0k");
    }
}
