//! normalize_usage — convert raw API response JSON into CanonicalUsage.
//!
//! Hermes equivalent: `normalize_usage()` in `usage_pricing.py`.
//!
//! Three API shapes are handled:
//!   1. OpenAI shape:    {"usage": {"prompt_tokens": N, "completion_tokens": N, ...}}
//!   2. Anthropic shape: {"usage": {"input_tokens": N, "output_tokens": N, "cache_read_input_tokens": N, ...}}
//!   3. ZhipuAI shape:   {"usage": {"prompt_tokens": N, "completion_tokens": N}} (OpenAI-compatible)

use crate::CanonicalUsage;
use serde_json::Value;

/// Extract a u64 from a JSON object by trying multiple key names.
/// Returns 0 if none of the keys are found or the value is not a number.
fn extract_u64(obj: &Value, keys: &[&str]) -> u64 {
    for key in keys {
        if let Some(v) = obj.get(key) {
            if let Some(n) = v.as_u64() {
                return n;
            }
        }
    }
    0
}

/// Normalize a raw LLM API response JSON into `CanonicalUsage`.
///
/// Accepts the full response body. The `usage` object can be at the top level
/// or nested under a `"usage"` key.
pub fn normalize_usage(raw: &Value) -> CanonicalUsage {
    // Try to find the usage sub-object.
    let usage_obj = raw.get("usage").unwrap_or(raw);

    let input_tokens = extract_u64(
        usage_obj,
        &[
            "input_tokens",  // Anthropic
            "prompt_tokens", // OpenAI / ZhipuAI
            "promptTokens",  // some adapters
        ],
    );

    let output_tokens = extract_u64(
        usage_obj,
        &[
            "output_tokens",     // Anthropic
            "completion_tokens", // OpenAI / ZhipuAI
            "completionTokens",
        ],
    );

    // Cache tokens — Anthropic uses these field names.
    let cache_read_tokens = extract_u64(
        usage_obj,
        &[
            "cache_read_input_tokens", // Anthropic
            "cached_tokens",           // OpenAI prompt_tokens_details.cached_tokens
            "cache_read_tokens",
        ],
    );

    // Also check nested prompt_tokens_details for OpenAI cached_tokens.
    let cache_read_tokens = if cache_read_tokens == 0 {
        if let Some(details) = usage_obj.get("prompt_tokens_details") {
            extract_u64(details, &["cached_tokens"])
        } else {
            0
        }
    } else {
        cache_read_tokens
    };

    let cache_write_tokens = extract_u64(
        usage_obj,
        &[
            "cache_creation_input_tokens", // Anthropic
            "cache_write_tokens",
        ],
    );

    // Reasoning tokens — OpenAI o-series.
    let reasoning_tokens = if let Some(details) = usage_obj.get("completion_tokens_details") {
        extract_u64(details, &["reasoning_tokens"])
    } else {
        extract_u64(usage_obj, &["reasoning_tokens"])
    };

    CanonicalUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_openai_shape() {
        let raw = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }
        });
        let u = normalize_usage(&raw);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
    }

    #[test]
    fn normalize_anthropic_shape() {
        let raw = json!({
            "usage": {
                "input_tokens": 200,
                "output_tokens": 80,
                "cache_read_input_tokens": 500,
                "cache_creation_input_tokens": 100
            }
        });
        let u = normalize_usage(&raw);
        assert_eq!(u.input_tokens, 200);
        assert_eq!(u.output_tokens, 80);
        assert_eq!(u.cache_read_tokens, 500);
        assert_eq!(u.cache_write_tokens, 100);
    }

    #[test]
    fn normalize_openai_with_cached_tokens() {
        let raw = json!({
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 200,
                "prompt_tokens_details": {
                    "cached_tokens": 800
                },
                "completion_tokens_details": {
                    "reasoning_tokens": 50
                }
            }
        });
        let u = normalize_usage(&raw);
        assert_eq!(u.input_tokens, 1000);
        assert_eq!(u.output_tokens, 200);
        assert_eq!(u.cache_read_tokens, 800);
        assert_eq!(u.reasoning_tokens, 50);
    }

    #[test]
    fn normalize_missing_fields_defaults_to_zero() {
        let raw = json!({});
        let u = normalize_usage(&raw);
        assert!(!u.has_data());
    }

    #[test]
    fn normalize_flat_usage_without_wrapper() {
        // Some providers return usage at the top level.
        let raw = json!({
            "input_tokens": 300,
            "output_tokens": 100
        });
        let u = normalize_usage(&raw);
        assert_eq!(u.input_tokens, 300);
        assert_eq!(u.output_tokens, 100);
    }
}
