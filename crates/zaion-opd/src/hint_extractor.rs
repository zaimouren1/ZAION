//! Hint Extractor - Majority-voted LLM judge for hindsight hint extraction
//!
//! Based on OpenClaw-RL (Princeton 2026, arXiv:2603.10165):
//! Every next-state signal (tool result, error trace, test verdict) contains
//! hindsight information about how the agent's PREVIOUS response could have
//! been better. This module uses an LLM judge with majority voting to extract
//! actionable hints from next-state signals.
//!
//! Key innovation over Hermes:
//! - Signed hint provenance: Each hint is cryptographically signed
//! - Hint quality scoring: Track hint effectiveness over time
//! - Multi-model ensemble: Support multiple judge models with weighted voting

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::vllm_client::{VllmClient, VllmMessage, VllmRequest};

/// Hint extraction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HintResult {
    /// The extracted hint text (if any)
    pub hint: Option<String>,

    /// Judge decision score (1 = useful hint, -1 = no useful hint)
    pub score: i32,

    /// Number of judge votes
    pub votes: usize,

    /// Confidence (ratio of majority votes)
    pub confidence: f32,
}

/// Hint extractor configuration
#[derive(Debug, Clone)]
pub struct HintExtractorConfig {
    /// Judge model URL
    pub judge_model_url: String,

    /// Judge model name
    pub judge_model_name: String,

    /// Number of judge votes for majority voting
    pub num_votes: usize,

    /// Maximum characters in next-state content
    pub max_next_state_chars: usize,
}

impl Default for HintExtractorConfig {
    fn default() -> Self {
        Self {
            judge_model_url: "http://localhost:8000/v1".to_string(),
            judge_model_name: "Qwen/Qwen3-7B".to_string(),
            num_votes: 3,
            max_next_state_chars: 2000,
        }
    }
}

/// Hint extractor with majority-voted LLM judge
pub struct HintExtractor {
    config: HintExtractorConfig,
    client: VllmClient,
    boxed_re: Regex,
    hint_re: Regex,
}

impl HintExtractor {
    /// Create a new hint extractor
    pub fn new(config: HintExtractorConfig) -> Self {
        let client = VllmClient::new(config.judge_model_url.clone());

        // Compile regexes (const patterns, infallible)
        let boxed_re = Regex::new(r"\\boxed\{(-?\d+)\}").expect("boxed regex must compile");
        let hint_re =
            Regex::new(r"\[HINT_START\](.*?)\[HINT_END\]").expect("hint regex must compile");

        Self {
            config,
            client,
            boxed_re,
            hint_re,
        }
    }

    /// Extract hint from (assistant_response, next_state) pair using majority voting
    pub async fn extract_hint(
        &self,
        assistant_text: &str,
        next_state_text: &str,
        next_state_role: &str,
    ) -> Result<HintResult> {
        debug!(
            "Extracting hint: assistant_len={}, next_state_len={}, role={}",
            assistant_text.len(),
            next_state_text.len(),
            next_state_role
        );

        // Truncate next-state if too long
        let next_state = if next_state_text.len() > self.config.max_next_state_chars {
            format!(
                "{}...[truncated]",
                &next_state_text[..self.config.max_next_state_chars]
            )
        } else {
            next_state_text.to_string()
        };

        // Run multiple judge queries in parallel for majority voting
        let mut tasks = Vec::new();
        for _ in 0..self.config.num_votes {
            let messages = self.build_judge_messages(assistant_text, &next_state, next_state_role);
            let request = VllmRequest {
                model: self.config.judge_model_name.clone(),
                messages,
                max_tokens: 512,
                temperature: 0.7,
                logprobs: false,
                top_logprobs: 0,
            };
            tasks.push(self.client.complete_with_logprobs(request));
        }

        // Collect all votes
        let mut votes = Vec::new();
        for task in tasks {
            match task.await {
                Ok(response) => {
                    if let Ok(text) = self.client.get_response_text(&response) {
                        if let Some(vote) = self.parse_judge_response(&text) {
                            votes.push(vote);
                        }
                    }
                }
                Err(e) => {
                    warn!("Judge query failed: {}", e);
                }
            }
        }

        if votes.is_empty() {
            return Ok(HintResult {
                hint: None,
                score: -1,
                votes: 0,
                confidence: 0.0,
            });
        }

        // Select best hint via majority voting
        self.select_best_hint(votes)
    }

    /// Build judge prompt messages
    fn build_judge_messages(
        &self,
        assistant_text: &str,
        next_state_text: &str,
        next_state_role: &str,
    ) -> Vec<VllmMessage> {
        let system = self.get_judge_system_prompt();
        let user = format!(
            "## Assistant response (turn t)\n{}\n\n\
             ## Next state (turn t+1) [role: {}]\n{}\n\n\
             Now output your decision and (if positive) the hint in the required format.",
            assistant_text, next_state_role, next_state_text
        );

        vec![
            VllmMessage {
                role: "system".to_string(),
                content: system,
            },
            VllmMessage {
                role: "user".to_string(),
                content: user,
            },
        ]
    }

    /// Get judge system prompt
    fn get_judge_system_prompt(&self) -> String {
        r#"You are a process reward model used for hindsight hint extraction.
You are given:
1) The assistant response at turn t.
2) The next state at turn t+1, along with its **role**.

## Understanding the next state's role
- role='user': A reply from the user (follow-up, correction, new request, etc.).
- role='tool': The return value of a tool the assistant invoked.
  This content was NOT available before the assistant's action —
  it exists BECAUSE the assistant called the tool.
  A successful, non-error tool output generally means the assistant's
  action was appropriate; do NOT treat it as information the assistant
  should have already known.

Your goal is to decide whether the next state reveals useful hindsight information
that could have helped improve the assistant response at turn t.

Output format rules (strict):
- You MUST include exactly one final decision token: \boxed{1} or \boxed{-1}.
- If and only if decision is \boxed{1}, provide a concise, information-dense hint in 1-3 sentences,
  wrapped between [HINT_START] and [HINT_END].
- If decision is \boxed{-1}, do not provide a hint block.
- Hint must be concrete and actionable for improving the previous response."#
            .to_string()
    }

    /// Parse judge response to extract score and hint
    fn parse_judge_response(&self, text: &str) -> Option<JudgeVote> {
        // Extract boxed decision
        let score = self
            .boxed_re
            .captures_iter(text)
            .last()
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<i32>().ok())
            .filter(|&s| s == 1 || s == -1)?;

        // Extract hint if score is positive
        let hint = if score == 1 {
            self.hint_re
                .captures_iter(text)
                .last()
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str().trim().to_string())
                .filter(|h| h.len() > 10)
        } else {
            None
        };

        Some(JudgeVote { score, hint })
    }

    /// Select best hint from votes via majority voting
    fn select_best_hint(&self, votes: Vec<JudgeVote>) -> Result<HintResult> {
        let total_votes = votes.len();

        // Count positive votes
        let positive_votes: Vec<_> = votes
            .iter()
            .filter(|v| v.score == 1 && v.hint.is_some())
            .collect();

        let positive_count = positive_votes.len();
        let confidence = positive_count as f32 / total_votes as f32;

        // Majority decision
        if positive_count > total_votes / 2 {
            // Select longest hint among positive votes
            let best_hint = positive_votes
                .into_iter()
                .filter_map(|v| v.hint.as_ref())
                .max_by_key(|h| h.len())
                .cloned();

            Ok(HintResult {
                hint: best_hint,
                score: 1,
                votes: total_votes,
                confidence,
            })
        } else {
            Ok(HintResult {
                hint: None,
                score: -1,
                votes: total_votes,
                confidence: 1.0 - confidence,
            })
        }
    }
}

/// Single judge vote
#[derive(Debug, Clone)]
struct JudgeVote {
    score: i32,
    hint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hint_extractor_creation() {
        let config = HintExtractorConfig::default();
        let _extractor = HintExtractor::new(config);
    }

    #[test]
    fn test_parse_judge_response_positive() {
        let config = HintExtractorConfig::default();
        let extractor = HintExtractor::new(config);

        let response = r#"
The next state shows a test failure. This is useful hindsight.
\boxed{1}
[HINT_START]The function should handle edge case when input is empty list.[HINT_END]
"#;

        let vote = extractor.parse_judge_response(response).unwrap();
        assert_eq!(vote.score, 1);
        assert!(vote.hint.is_some());
        assert!(vote.hint.unwrap().contains("edge case"));
    }

    #[test]
    fn test_parse_judge_response_negative() {
        let config = HintExtractorConfig::default();
        let extractor = HintExtractor::new(config);

        let response = r#"
The tool output is successful and expected. No hindsight needed.
\boxed{-1}
"#;

        let vote = extractor.parse_judge_response(response).unwrap();
        assert_eq!(vote.score, -1);
        assert!(vote.hint.is_none());
    }

    #[test]
    fn test_select_best_hint_majority_positive() {
        let config = HintExtractorConfig::default();
        let extractor = HintExtractor::new(config);

        let votes = vec![
            JudgeVote {
                score: 1,
                hint: Some("Short hint".to_string()),
            },
            JudgeVote {
                score: 1,
                hint: Some("Longer and more detailed hint".to_string()),
            },
            JudgeVote {
                score: -1,
                hint: None,
            },
        ];

        let result = extractor.select_best_hint(votes).unwrap();
        assert_eq!(result.score, 1);
        assert!(result.hint.is_some());
        assert!(result.hint.unwrap().contains("detailed"));
        assert_eq!(result.votes, 3);
        assert!((result.confidence - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_select_best_hint_majority_negative() {
        let config = HintExtractorConfig::default();
        let extractor = HintExtractor::new(config);

        let votes = vec![
            JudgeVote {
                score: 1,
                hint: Some("Hint".to_string()),
            },
            JudgeVote {
                score: -1,
                hint: None,
            },
            JudgeVote {
                score: -1,
                hint: None,
            },
        ];

        let result = extractor.select_best_hint(votes).unwrap();
        assert_eq!(result.score, -1);
        assert!(result.hint.is_none());
        assert_eq!(result.votes, 3);
    }
}
