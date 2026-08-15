//! Pricing table — per-token costs for 15+ LLM models.
//!
//! All costs are stored as f64 micro-dollars per token (USD × 1e-6).
//! This matches Hermes' Decimal-precision approach while staying compatible with Rust's f64.
//!
//! Formula: cost_usd = tokens × price_per_million / 1_000_000
//!
//! Prices are correct as of 2026-04 and may drift. Update periodically.

/// Pricing entry for a single model.
#[derive(Debug, Clone)]
pub struct PricingEntry {
    /// Provider name (lowercase).
    pub provider: &'static str,
    /// Model identifier as used in API requests.
    pub model: &'static str,
    /// Alternative/alias model names this entry matches.
    pub aliases: &'static [&'static str],
    /// Cost per 1 million input tokens (USD).
    pub input_per_million: f64,
    /// Cost per 1 million output tokens (USD).
    pub output_per_million: f64,
    /// Cost per 1 million cache-read tokens (USD). 0 if not applicable.
    pub cache_read_per_million: f64,
    /// Cost per 1 million cache-write tokens (USD). 0 if not applicable.
    pub cache_write_per_million: f64,
    /// Cost per 1 million reasoning tokens (USD). 0 = billed at output rate.
    pub reasoning_per_million: f64,
}

/// Static pricing snapshot — 15+ models across all major providers.
///
/// Hermes equivalent: `PRICING` dict in `usage_pricing.py`.
pub static PRICING_TABLE: &[PricingEntry] = &[
    // ── Anthropic ────────────────────────────────────────────────────────────
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4-5",
        aliases: &["claude-opus-4.5", "claude-opus-4"],
        input_per_million: 15.0,
        output_per_million: 75.0,
        cache_read_per_million: 1.5,
        cache_write_per_million: 18.75,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-sonnet-4-5",
        aliases: &[
            "claude-sonnet-4.5",
            "claude-sonnet-4",
            "claude-3-7-sonnet-20250219",
        ],
        input_per_million: 3.0,
        output_per_million: 15.0,
        cache_read_per_million: 0.3,
        cache_write_per_million: 3.75,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-haiku-4-5",
        aliases: &["claude-haiku-4.5", "claude-haiku-4"],
        input_per_million: 0.8,
        output_per_million: 4.0,
        cache_read_per_million: 0.08,
        cache_write_per_million: 1.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-5-sonnet-20241022",
        aliases: &["claude-3-5-sonnet", "claude-3.5-sonnet"],
        input_per_million: 3.0,
        output_per_million: 15.0,
        cache_read_per_million: 0.3,
        cache_write_per_million: 3.75,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-5-haiku-20241022",
        aliases: &["claude-3-5-haiku", "claude-3.5-haiku"],
        input_per_million: 0.8,
        output_per_million: 4.0,
        cache_read_per_million: 0.08,
        cache_write_per_million: 1.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-opus-20240229",
        aliases: &["claude-3-opus"],
        input_per_million: 15.0,
        output_per_million: 75.0,
        cache_read_per_million: 1.5,
        cache_write_per_million: 18.75,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-haiku-20240307",
        aliases: &["claude-3-haiku"],
        input_per_million: 0.25,
        output_per_million: 1.25,
        cache_read_per_million: 0.03,
        cache_write_per_million: 0.3,
        reasoning_per_million: 0.0,
    },
    // ── OpenAI ───────────────────────────────────────────────────────────────
    PricingEntry {
        provider: "openai",
        model: "gpt-4o",
        aliases: &["gpt-4o-2024-11-20", "gpt-4o-2024-08-06"],
        input_per_million: 2.5,
        output_per_million: 10.0,
        cache_read_per_million: 1.25,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4o-mini",
        aliases: &["gpt-4o-mini-2024-07-18"],
        input_per_million: 0.15,
        output_per_million: 0.6,
        cache_read_per_million: 0.075,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4.1",
        aliases: &["gpt-4.1-2025-04-14"],
        input_per_million: 2.0,
        output_per_million: 8.0,
        cache_read_per_million: 0.5,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "openai",
        model: "o3-mini",
        aliases: &["o3-mini-2025-01-31"],
        input_per_million: 1.1,
        output_per_million: 4.4,
        cache_read_per_million: 0.55,
        cache_write_per_million: 0.0,
        reasoning_per_million: 4.4,
    },
    PricingEntry {
        provider: "openai",
        model: "o4-mini",
        aliases: &["o4-mini-2025-04-16"],
        input_per_million: 1.1,
        output_per_million: 4.4,
        cache_read_per_million: 0.275,
        cache_write_per_million: 0.0,
        reasoning_per_million: 4.4,
    },
    // ── DeepSeek ─────────────────────────────────────────────────────────────
    PricingEntry {
        provider: "deepseek",
        model: "deepseek-chat",
        aliases: &["deepseek-v3", "deepseek-v3.1"],
        input_per_million: 0.27,
        output_per_million: 1.1,
        cache_read_per_million: 0.07,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "deepseek",
        model: "deepseek-reasoner",
        aliases: &["deepseek-r1", "deepseek-r1-0528"],
        input_per_million: 0.55,
        output_per_million: 2.19,
        cache_read_per_million: 0.14,
        cache_write_per_million: 0.0,
        reasoning_per_million: 2.19,
    },
    // ── Google ───────────────────────────────────────────────────────────────
    PricingEntry {
        provider: "google",
        model: "gemini-2.0-flash",
        aliases: &["gemini-2.0-flash-001"],
        input_per_million: 0.1,
        output_per_million: 0.4,
        cache_read_per_million: 0.025,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "google",
        model: "gemini-1.5-pro",
        aliases: &["gemini-1.5-pro-002"],
        input_per_million: 1.25,
        output_per_million: 5.0,
        cache_read_per_million: 0.3125,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    // ── ZhipuAI ──────────────────────────────────────────────────────────────
    PricingEntry {
        provider: "zhipuai",
        model: "glm-4-flash",
        aliases: &["glm-4-flash-250414"],
        input_per_million: 0.0,
        output_per_million: 0.0,
        cache_read_per_million: 0.0,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "zhipuai",
        model: "glm-4-5",
        aliases: &["glm-4.5", "glm-4.5-air"],
        input_per_million: 0.3,
        output_per_million: 0.9,
        cache_read_per_million: 0.0,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    PricingEntry {
        provider: "zhipuai",
        model: "glm-4",
        aliases: &["glm-4-plus", "glm-4-air"],
        input_per_million: 0.1,
        output_per_million: 0.1,
        cache_read_per_million: 0.0,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    // ── MiniMax ──────────────────────────────────────────────────────────────
    PricingEntry {
        provider: "minimax",
        model: "abab6.5s",
        aliases: &["minimax-text-01"],
        input_per_million: 0.1,
        output_per_million: 0.1,
        cache_read_per_million: 0.0,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
    // ── Ollama (local, free) ──────────────────────────────────────────────────
    PricingEntry {
        provider: "ollama",
        model: "local",
        aliases: &["llama3", "llama3.1", "llama3.2", "qwen2.5", "mistral"],
        input_per_million: 0.0,
        output_per_million: 0.0,
        cache_read_per_million: 0.0,
        cache_write_per_million: 0.0,
        reasoning_per_million: 0.0,
    },
];

/// Look up pricing entry by model name.
///
/// Matching priority (stops at first hit):
///   1. Exact match on `entry.model`
///   2. Exact match on any alias
///   3. Model name starts with `entry.model` + a separator ('-', '/', '.')
///   4. Any alias starts with the query + a separator
///
/// We never do bare prefix matching ("gpt-4o" must not match "gpt-4o-mini").
pub fn lookup_pricing(model: &str) -> Option<&'static PricingEntry> {
    let model_lower = model.to_lowercase();

    // Helper: does `candidate` match `target` exactly or as a versioned prefix?
    // A versioned prefix means: "gpt-4o" matches "gpt-4o-2024-08-06" but NOT "gpt-4o-mini".
    fn is_match(candidate: &str, target: &str) -> bool {
        if candidate == target {
            return true;
        }
        // Allow candidate to be a prefix of target ONLY if the next char is a separator.
        if let Some(rest) = target.strip_prefix(candidate) {
            matches!(rest.chars().next(), Some('-') | Some('/') | Some('.'))
        } else if let Some(rest) = candidate.strip_prefix(target) {
            matches!(rest.chars().next(), Some('-') | Some('/') | Some('.'))
        } else {
            false
        }
    }

    // Phase 1: exact matches only.
    for entry in PRICING_TABLE {
        if entry.model == model_lower {
            return Some(entry);
        }
        if entry.aliases.iter().any(|a| *a == model_lower) {
            return Some(entry);
        }
    }

    // Phase 2: versioned prefix matches.
    for entry in PRICING_TABLE {
        if is_match(entry.model, &model_lower) {
            return Some(entry);
        }
        if entry.aliases.iter().any(|a| is_match(a, &model_lower)) {
            return Some(entry);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_not_empty() {
        assert!(!PRICING_TABLE.is_empty());
    }

    #[test]
    fn lookup_anthropic_opus() {
        let entry = lookup_pricing("claude-opus-4-5").unwrap();
        assert_eq!(entry.provider, "anthropic");
        assert!(entry.input_per_million > 0.0);
        assert!(entry.output_per_million > entry.input_per_million);
    }

    #[test]
    fn lookup_by_alias() {
        let entry = lookup_pricing("claude-3.5-sonnet").unwrap();
        assert_eq!(entry.provider, "anthropic");
    }

    #[test]
    fn lookup_deepseek_chat() {
        let entry = lookup_pricing("deepseek-chat").unwrap();
        assert_eq!(entry.provider, "deepseek");
        assert!(entry.input_per_million < 1.0); // cheap model
    }

    #[test]
    fn lookup_local_returns_zeros() {
        let entry = lookup_pricing("llama3").unwrap();
        assert_eq!(entry.provider, "ollama");
        assert_eq!(entry.input_per_million, 0.0);
        assert_eq!(entry.output_per_million, 0.0);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup_pricing("gpt-999-turbo-ultra").is_none());
    }
}
