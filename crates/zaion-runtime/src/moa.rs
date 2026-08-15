//! Mixture-of-Agents (MoA) engine.
//!
//! Hermes equivalent: `agent/mixture_of_agents.py`.
//!
//! Architecture:
//!   1. Proposer round: N LLM providers answer the same query in parallel
//!   2. Aggregator: a single "strong" model synthesizes all responses into one answer
//!
//! This module handles the aggregation logic and response formatting.
//! Actual LLM calls are delegated to the `LlmProvider` trait so the engine
//! stays provider-agnostic and testable.

use serde::{Deserialize, Serialize};

/// Configuration for a single MoA proposer (one model slot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoaProposer {
    /// Display name (used in aggregator prompt).
    pub name: String,
    /// Provider type: "anthropic" | "openai" | etc.
    pub provider: String,
    /// Model name to use for this slot.
    pub model: String,
}

/// Configuration for the MoA engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoaConfig {
    /// Proposer slots (run in parallel).
    pub proposers: Vec<MoaProposer>,
    /// Aggregator model (receives all proposals, produces final answer).
    pub aggregator_provider: String,
    pub aggregator_model: String,
    /// Max tokens per proposer response.
    pub proposer_max_tokens: usize,
    /// Max tokens for the aggregator response.
    pub aggregator_max_tokens: usize,
    /// Whether to include proposer reasoning in the final output (verbose mode).
    pub include_proposer_details: bool,
}

impl Default for MoaConfig {
    fn default() -> Self {
        Self {
            proposers: vec![
                MoaProposer {
                    name: "Claude-Sonnet".into(),
                    provider: "anthropic".into(),
                    model: "claude-sonnet-4-5".into(),
                },
                MoaProposer {
                    name: "GPT-4o-mini".into(),
                    provider: "openai".into(),
                    model: "gpt-4o-mini".into(),
                },
                MoaProposer {
                    name: "DeepSeek-Chat".into(),
                    provider: "openai".into(),
                    model: "deepseek-chat".into(),
                },
                MoaProposer {
                    name: "GLM-4-Flash".into(),
                    provider: "zhipuai".into(),
                    model: "glm-4-flash".into(),
                },
            ],
            aggregator_provider: "anthropic".into(),
            aggregator_model: "claude-opus-4-5".into(),
            proposer_max_tokens: 2048,
            aggregator_max_tokens: 4096,
            include_proposer_details: false,
        }
    }
}

/// Result from a single proposer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposerResult {
    pub proposer_name: String,
    pub model: String,
    pub content: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub error: Option<String>,
}

/// Final aggregated result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoaResult {
    /// The aggregated final answer.
    pub answer: String,
    /// Individual proposer results (for transparency/debugging).
    pub proposals: Vec<ProposerResult>,
    /// Total input tokens across all calls.
    pub total_tokens_in: u32,
    /// Total output tokens across all calls.
    pub total_tokens_out: u32,
}

/// Build the aggregator prompt from proposals.
///
/// This is the standard Hermes MoA prompt format:
///   "You have received N responses from expert AI models. Synthesize..."
pub fn build_aggregator_prompt(query: &str, proposals: &[ProposerResult]) -> String {
    let mut parts = Vec::new();
    parts.push(format!(
        "You are an expert AI orchestrator. You have received {} responses from specialized AI models \
         answering the same query. Your task is to synthesize the best possible answer by:\n\
         1. Identifying the most accurate and complete information across all responses\n\
         2. Resolving any contradictions using your own judgment\n\
         3. Producing a comprehensive, well-structured final answer\n\n\
         Original query: {}\n\n\
         --- Responses from proposer models ---",
        proposals.len(), query
    ));

    for (i, prop) in proposals.iter().enumerate() {
        if prop.error.is_some() {
            parts.push(format!(
                "\n[Response {} — {} (ERROR: {})]:\n(unavailable)",
                i + 1,
                prop.proposer_name,
                prop.error.as_deref().unwrap_or("unknown")
            ));
        } else {
            parts.push(format!(
                "\n[Response {} — {}]:\n{}",
                i + 1,
                prop.proposer_name,
                prop.content
            ));
        }
    }

    parts.push(
        "\n--- End of responses ---\n\n\
         Now provide the best synthesized answer. Be comprehensive and accurate. \
         Do not mention that you are aggregating — just give the best answer directly."
            .into(),
    );

    parts.join("\n")
}

/// Extract the best single proposal when the aggregator is unavailable.
///
/// Preference order: longest non-empty response (heuristic for most detailed).
pub fn best_fallback_proposal(proposals: &[ProposerResult]) -> String {
    proposals
        .iter()
        .filter(|p| p.error.is_none() && !p.content.is_empty())
        .max_by_key(|p| p.content.len())
        .map(|p| p.content.clone())
        .unwrap_or_else(|| "No proposals available.".into())
}

/// Format MoA result for display.
pub fn format_moa_output(result: &MoaResult, verbose: bool) -> String {
    let mut out = result.answer.clone();
    if verbose {
        out.push_str("\n\n---\n**MoA Proposer Details:**\n");
        for p in &result.proposals {
            if let Some(ref err) = p.error {
                out.push_str(&format!(
                    "- {} ({}): ERROR — {}\n",
                    p.proposer_name, p.model, err
                ));
            } else {
                out.push_str(&format!(
                    "- {} ({}): {} tokens in, {} tokens out\n",
                    p.proposer_name, p.model, p.tokens_in, p.tokens_out
                ));
            }
        }
        out.push_str(&format!(
            "Total: {} in, {} out\n",
            result.total_tokens_in, result.total_tokens_out
        ));
    }
    out
}

/// Callable MoA runner that works with any `LlmProvider`-compatible call function.
///
/// `call_llm` signature: `(provider: &str, model: &str, prompt: &str, max_tokens: usize) -> Result<(String, u32, u32), String>`
///   where the tuple is `(content, tokens_in, tokens_out)`.
///
/// This design keeps MoA independent of the async runtime and concrete providers.
pub fn run_moa_sync<F>(query: &str, config: &MoaConfig, call_llm: F) -> MoaResult
where
    F: Fn(&str, &str, &str, usize) -> Result<(String, u32, u32), String>,
{
    // Run proposers (sequential in sync mode; parallel in async mode via caller)
    let proposals: Vec<ProposerResult> = config
        .proposers
        .iter()
        .map(
            |p| match call_llm(&p.provider, &p.model, query, config.proposer_max_tokens) {
                Ok((content, tokens_in, tokens_out)) => ProposerResult {
                    proposer_name: p.name.clone(),
                    model: p.model.clone(),
                    content,
                    tokens_in,
                    tokens_out,
                    error: None,
                },
                Err(e) => ProposerResult {
                    proposer_name: p.name.clone(),
                    model: p.model.clone(),
                    content: String::new(),
                    tokens_in: 0,
                    tokens_out: 0,
                    error: Some(e),
                },
            },
        )
        .collect();

    // Aggregate
    let agg_prompt = build_aggregator_prompt(query, &proposals);
    let (answer, agg_in, agg_out) = match call_llm(
        &config.aggregator_provider,
        &config.aggregator_model,
        &agg_prompt,
        config.aggregator_max_tokens,
    ) {
        Ok(r) => r,
        Err(_) => {
            // Fallback: pick best single proposal
            (best_fallback_proposal(&proposals), 0, 0)
        }
    };

    let total_in = proposals.iter().map(|p| p.tokens_in).sum::<u32>() + agg_in;
    let total_out = proposals.iter().map(|p| p.tokens_out).sum::<u32>() + agg_out;

    MoaResult {
        answer,
        proposals,
        total_tokens_in: total_in,
        total_tokens_out: total_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_call(
        content: &'static str,
    ) -> impl Fn(&str, &str, &str, usize) -> Result<(String, u32, u32), String> {
        move |_provider, _model, _prompt, _max| Ok((content.to_string(), 100, 50))
    }

    fn mock_call_fail() -> impl Fn(&str, &str, &str, usize) -> Result<(String, u32, u32), String> {
        |_provider, _model, _prompt, _max| Err("provider unavailable".to_string())
    }

    fn small_config() -> MoaConfig {
        MoaConfig {
            proposers: vec![
                MoaProposer {
                    name: "A".into(),
                    provider: "p1".into(),
                    model: "m1".into(),
                },
                MoaProposer {
                    name: "B".into(),
                    provider: "p2".into(),
                    model: "m2".into(),
                },
            ],
            aggregator_provider: "agg_p".into(),
            aggregator_model: "agg_m".into(),
            proposer_max_tokens: 100,
            aggregator_max_tokens: 200,
            include_proposer_details: false,
        }
    }

    #[test]
    fn run_moa_with_successful_proposers() {
        let cfg = small_config();
        let result = run_moa_sync("what is 2+2?", &cfg, mock_call("The answer is 4."));
        // Aggregator also runs and returns same mock content
        assert!(!result.answer.is_empty());
        assert_eq!(result.proposals.len(), 2);
        assert!(result.proposals.iter().all(|p| p.error.is_none()));
    }

    #[test]
    fn run_moa_falls_back_on_aggregator_failure() {
        let cfg = small_config();
        // Proposers succeed, aggregator fails
        let call_count = std::sync::atomic::AtomicUsize::new(0);
        let result = run_moa_sync("query", &cfg, |provider, _model, _prompt, _max| {
            let n = call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if provider == "agg_p" {
                // aggregator call
                Err("aggregator down".into())
            } else {
                Ok((format!("proposal-{}", n), 10, 5))
            }
        });
        // Should fall back to best proposal
        assert!(!result.answer.is_empty());
        assert!(result.answer.contains("proposal"));
    }

    #[test]
    fn run_moa_all_proposers_fail_returns_fallback_message() {
        let cfg = small_config();
        let result = run_moa_sync("query", &cfg, mock_call_fail());
        // All proposals fail, aggregator gets empty proposals, aggregator also fails → fallback message
        assert_eq!(result.answer, "No proposals available.");
    }

    #[test]
    fn aggregator_prompt_contains_all_proposals() {
        let proposals = vec![
            ProposerResult {
                proposer_name: "A".into(),
                model: "m1".into(),
                content: "ans A".into(),
                tokens_in: 1,
                tokens_out: 1,
                error: None,
            },
            ProposerResult {
                proposer_name: "B".into(),
                model: "m2".into(),
                content: "ans B".into(),
                tokens_in: 1,
                tokens_out: 1,
                error: None,
            },
        ];
        let prompt = build_aggregator_prompt("my query", &proposals);
        assert!(prompt.contains("ans A"));
        assert!(prompt.contains("ans B"));
        assert!(prompt.contains("my query"));
        assert!(prompt.contains("2 responses"));
    }

    #[test]
    fn best_fallback_picks_longest_response() {
        let proposals = vec![
            ProposerResult {
                proposer_name: "A".into(),
                model: "m".into(),
                content: "short".into(),
                tokens_in: 1,
                tokens_out: 1,
                error: None,
            },
            ProposerResult {
                proposer_name: "B".into(),
                model: "m".into(),
                content: "this is a longer response".into(),
                tokens_in: 1,
                tokens_out: 1,
                error: None,
            },
            ProposerResult {
                proposer_name: "C".into(),
                model: "m".into(),
                content: String::new(),
                tokens_in: 0,
                tokens_out: 0,
                error: Some("err".into()),
            },
        ];
        let best = best_fallback_proposal(&proposals);
        assert_eq!(best, "this is a longer response");
    }

    #[test]
    fn format_moa_output_verbose_includes_details() {
        let result = MoaResult {
            answer: "Final answer".into(),
            proposals: vec![ProposerResult {
                proposer_name: "A".into(),
                model: "m1".into(),
                content: "x".into(),
                tokens_in: 100,
                tokens_out: 50,
                error: None,
            }],
            total_tokens_in: 100,
            total_tokens_out: 50,
        };
        let out = format_moa_output(&result, true);
        assert!(out.contains("Final answer"));
        assert!(out.contains("MoA Proposer Details"));
        assert!(out.contains("100 tokens in"));
    }

    #[test]
    fn format_moa_output_non_verbose_excludes_details() {
        let result = MoaResult {
            answer: "Final answer".into(),
            proposals: vec![],
            total_tokens_in: 0,
            total_tokens_out: 0,
        };
        let out = format_moa_output(&result, false);
        assert_eq!(out, "Final answer");
    }

    #[test]
    fn moa_config_default_has_four_proposers() {
        let cfg = MoaConfig::default();
        assert_eq!(cfg.proposers.len(), 4);
    }

    #[test]
    fn moa_token_totals_are_summed() {
        let cfg = small_config();
        let result = run_moa_sync("query", &cfg, mock_call("answer"));
        // 2 proposers × 100 in each + 1 aggregator × 100 in = 300 in total
        assert_eq!(result.total_tokens_in, 300);
        assert_eq!(result.total_tokens_out, 150);
    }
}
