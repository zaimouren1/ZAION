//! Cost estimation — convert CanonicalUsage + model into USD cost breakdown.
//!
//! Hermes equivalent: `estimate_cost()` in `usage_pricing.py`.

use crate::{lookup_pricing, CanonicalUsage};

/// Detailed cost breakdown for one LLM call.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostResult {
    /// Provider of the model used.
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// Cost of prompt (input) tokens in USD.
    pub input_cost_usd: f64,
    /// Cost of completion (output) tokens in USD.
    pub output_cost_usd: f64,
    /// Cost of cache-read tokens in USD.
    pub cache_read_cost_usd: f64,
    /// Cost of cache-write tokens in USD.
    pub cache_write_cost_usd: f64,
    /// Cost of reasoning tokens in USD.
    pub reasoning_cost_usd: f64,
    /// Total cost in USD (sum of all above).
    pub total_cost_usd: f64,
    /// Effective savings from cache read vs full input price.
    pub cache_savings_usd: f64,
}

impl CostResult {
    /// Format total cost as a human-readable string.
    ///
    /// Uses 6-digit precision for micro-costs (< $0.001), 4-digit precision
    /// otherwise — the larger range collapses into a single branch.
    pub fn format_total(&self) -> String {
        if self.total_cost_usd < 0.001 {
            format!("${:.6}", self.total_cost_usd)
        } else {
            format!("${:.4}", self.total_cost_usd)
        }
    }
}

/// Estimate cost for a given usage + model.
///
/// Returns `None` if the model is not in the pricing table.
/// Cache savings are calculated as: (cache_read_tokens × input_rate) - (cache_read_tokens × cache_read_rate)
pub fn estimate_usage_cost(usage: &CanonicalUsage, model: &str) -> Option<CostResult> {
    let entry = lookup_pricing(model)?;
    let per_m = 1_000_000.0_f64;

    let input_cost = usage.input_tokens as f64 * entry.input_per_million / per_m;
    let output_cost = usage.output_tokens as f64 * entry.output_per_million / per_m;
    let cache_read_cost = usage.cache_read_tokens as f64 * entry.cache_read_per_million / per_m;
    let cache_write_cost = usage.cache_write_tokens as f64 * entry.cache_write_per_million / per_m;

    // Reasoning tokens: billed at reasoning_per_million if set, else output rate.
    let reasoning_rate = if entry.reasoning_per_million > 0.0 {
        entry.reasoning_per_million
    } else {
        entry.output_per_million
    };
    let reasoning_cost = usage.reasoning_tokens as f64 * reasoning_rate / per_m;

    // Cache savings: difference between reading at input price vs cache price.
    let cache_savings = if entry.cache_read_per_million > 0.0 {
        usage.cache_read_tokens as f64 * (entry.input_per_million - entry.cache_read_per_million)
            / per_m
    } else {
        0.0
    };

    let total = input_cost + output_cost + cache_read_cost + cache_write_cost + reasoning_cost;

    Some(CostResult {
        provider: entry.provider.to_string(),
        model: model.to_string(),
        input_cost_usd: input_cost,
        output_cost_usd: output_cost,
        cache_read_cost_usd: cache_read_cost,
        cache_write_cost_usd: cache_write_cost,
        reasoning_cost_usd: reasoning_cost,
        total_cost_usd: total,
        cache_savings_usd: cache_savings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64) -> CanonicalUsage {
        CanonicalUsage {
            input_tokens: input,
            output_tokens: output,
            ..Default::default()
        }
    }

    #[test]
    fn gpt4o_mini_cost() {
        // 1M input + 1M output
        let u = usage(1_000_000, 1_000_000);
        let cost = estimate_usage_cost(&u, "gpt-4o-mini").unwrap();
        assert_eq!(cost.provider, "openai");
        // $0.15 + $0.60 = $0.75
        assert!((cost.total_cost_usd - 0.75).abs() < 1e-9);
    }

    #[test]
    fn anthropic_opus_with_cache() {
        let u = CanonicalUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 5000,
            cache_write_tokens: 1000,
            reasoning_tokens: 0,
        };
        let cost = estimate_usage_cost(&u, "claude-opus-4-5").unwrap();
        // input: 1000 * 15.0 / 1M = 0.000015
        // output: 500 * 75.0 / 1M = 0.0000375
        // cache_read: 5000 * 1.5 / 1M = 0.0000075
        // cache_write: 1000 * 18.75 / 1M = 0.000018750
        let expected =
            1000.0 * 15.0 / 1e6 + 500.0 * 75.0 / 1e6 + 5000.0 * 1.5 / 1e6 + 1000.0 * 18.75 / 1e6;
        assert!((cost.total_cost_usd - expected).abs() < 1e-9);
        assert!(cost.cache_savings_usd > 0.0);
    }

    #[test]
    fn deepseek_chat_cheap() {
        let u = usage(100_000, 50_000);
        let cost = estimate_usage_cost(&u, "deepseek-chat").unwrap();
        assert!(cost.total_cost_usd < 0.1); // very cheap model
    }

    #[test]
    fn o3_mini_reasoning_tokens() {
        let u = CanonicalUsage {
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_tokens: 2000,
            ..Default::default()
        };
        let cost = estimate_usage_cost(&u, "o3-mini").unwrap();
        // reasoning billed at reasoning_per_million = 4.4 (same as output)
        assert!(cost.reasoning_cost_usd > 0.0);
    }

    #[test]
    fn local_model_zero_cost() {
        let u = usage(100_000, 50_000);
        let cost = estimate_usage_cost(&u, "llama3").unwrap();
        assert_eq!(cost.total_cost_usd, 0.0);
    }

    #[test]
    fn unknown_model_returns_none() {
        let u = usage(1000, 500);
        assert!(estimate_usage_cost(&u, "unknown-model-xyz").is_none());
    }

    #[test]
    fn format_total_small_value() {
        let cost = CostResult {
            total_cost_usd: 0.0001234,
            provider: "test".into(),
            model: "m".into(),
            ..Default::default()
        };
        let s = cost.format_total();
        assert!(s.starts_with('$'));
    }

    #[test]
    fn glm4_flash_free() {
        let u = usage(10_000, 5_000);
        let cost = estimate_usage_cost(&u, "glm-4-flash").unwrap();
        assert_eq!(cost.total_cost_usd, 0.0);
    }
}
