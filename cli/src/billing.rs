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

        if let Some(section) = section && let Some(user_models) = &section.models {
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

    /// Estimate cost for a single provider response.
    ///
    /// If the model has no configured prices, returns `None`. When cache
    /// hit/miss breakdowns are missing, all input tokens are billed at the
    /// cache-miss rate.
    pub fn estimate(&self, model: Option<&str>, usage: &TokenUsage) -> Option<CostEstimate> {
        let model = model?;
        let pricing = self.models.get(model)?;

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
}
