use crate::RuntimeError;
use serde::{Deserialize, Serialize};
/// TTC — Test-Time Compute Dynamic Allocator (Patch TTC, Godkiller Blueprint)
///
/// Implements System-1 (fast thinking) vs System-2 (slow thinking) dynamic switching.
/// Complexity estimation drives automatic compute budget allocation.
///
/// Architecture:
///   ComplexityEstimator → ComplexityScore → DynamicComputeAllocator → ThinkingMode
///   ThinkingMode::Fast  → single LLM call
///   ThinkingMode::Deep  → MctsPlanner multi-path reasoning
use std::collections::HashMap;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

// ── Thinking Mode ────────────────────────────────────────────────────────────

/// System-1 = fast / System-2 = deep multi-path
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ThinkingMode {
    /// Single LLM call, <500ms target
    Fast,
    /// Multi-path MCTS + candidate evaluation
    Deep {
        max_paths: usize,
        token_budget: usize,
    },
}

impl ThinkingMode {
    pub fn is_deep(self) -> bool {
        matches!(self, ThinkingMode::Deep { .. })
    }

    pub fn token_budget(self) -> usize {
        match self {
            ThinkingMode::Fast => 2_000,
            ThinkingMode::Deep { token_budget, .. } => token_budget,
        }
    }
}

// ── Complexity Score ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityScore {
    /// Normalised 0.0–1.0
    pub score: f32,
    /// Driving factors (for transparency / ledger audit)
    pub factors: HashMap<String, f32>,
}

impl ComplexityScore {
    pub fn new(score: f32, factors: HashMap<String, f32>) -> Self {
        ComplexityScore {
            score: score.clamp(0.0, 1.0),
            factors,
        }
    }

    /// Threshold below which we use System-1
    const FAST_THRESHOLD: f32 = 0.35;
    /// Threshold above which we use System-2 with maximum paths
    const DEEP_MAX_THRESHOLD: f32 = 0.75;

    pub fn to_mode(&self, forced: Option<ThinkingMode>) -> ThinkingMode {
        if let Some(m) = forced {
            return m;
        }
        if self.score < Self::FAST_THRESHOLD {
            ThinkingMode::Fast
        } else if self.score >= Self::DEEP_MAX_THRESHOLD {
            ThinkingMode::Deep {
                max_paths: 5,
                token_budget: 16_000,
            }
        } else {
            ThinkingMode::Deep {
                max_paths: 3,
                token_budget: 8_000,
            }
        }
    }
}

// ── Complexity Estimator ──────────────────────────────────────────────────────

/// Heuristic complexity estimator. Scoring is intentionally transparent —
/// every factor is logged so the ledger captures why System-2 was triggered.
pub struct ComplexityEstimator {
    /// Historical failure rate per task_type (loaded from ledger stats).
    failure_rates: HashMap<String, f32>,
}

impl ComplexityEstimator {
    pub fn new() -> Self {
        ComplexityEstimator {
            failure_rates: HashMap::new(),
        }
    }

    /// Feed historical failure rates (task_type → rate 0.0–1.0).
    pub fn with_failure_rates(mut self, rates: HashMap<String, f32>) -> Self {
        self.failure_rates = rates;
        self
    }

    /// Estimate complexity of a task given its type, input text, and optional token limit.
    pub fn estimate(
        &self,
        task_type: &str,
        input_text: &str,
        requested_budget: Option<usize>,
    ) -> ComplexityScore {
        let mut factors: HashMap<String, f32> = HashMap::new();

        // Factor 1: input length (normalised to 0-1 at 2000 chars = max)
        let length_factor = (input_text.len() as f32 / 2_000.0).min(1.0);
        factors.insert("input_length".into(), length_factor);

        // Factor 2: task type classification
        let type_factor = Self::classify_type(task_type);
        factors.insert("task_type".into(), type_factor);

        // Factor 3: question words suggesting multi-step reasoning
        let reasoning_factor = Self::detect_reasoning_markers(input_text);
        factors.insert("reasoning_markers".into(), reasoning_factor);

        // Factor 4: historical failure rate (if available)
        let failure_factor = self.failure_rates.get(task_type).copied().unwrap_or(0.0);
        factors.insert("historical_failure_rate".into(), failure_factor);

        // Factor 5: explicit budget hint (large budget → user wants deep thinking)
        let budget_factor = requested_budget
            .map(|b| (b as f32 / 16_000.0).min(1.0))
            .unwrap_or(0.0);
        factors.insert("explicit_budget".into(), budget_factor);

        // Weighted sum
        let score = 0.25 * length_factor
            + 0.30 * type_factor
            + 0.25 * reasoning_factor
            + 0.10 * failure_factor
            + 0.10 * budget_factor;

        ComplexityScore::new(score, factors)
    }

    fn classify_type(task_type: &str) -> f32 {
        match task_type {
            "chat" | "greeting" | "ping" => 0.05,
            "summarize" | "translate" | "classify" => 0.20,
            "qa" | "search" | "lookup" => 0.25,
            "code" | "debug" | "refactor" => 0.55,
            "design" | "plan" | "architecture" => 0.80,
            "research" | "analysis" | "synthesis" => 0.85,
            "multi-step" | "agentic" | "workflow" => 0.90,
            _ => 0.30,
        }
    }

    fn detect_reasoning_markers(text: &str) -> f32 {
        let markers = [
            "design",
            "architect",
            "distributed",
            "trade-off",
            "evaluate",
            "compare",
            "best approach",
            "why",
            "how should",
            "step by step",
            "complex",
            "multi",
            "strategy",
            "optimize",
            "analyze",
        ];
        let text_lower = text.to_lowercase();
        let hits = markers.iter().filter(|m| text_lower.contains(*m)).count();
        (hits as f32 / markers.len() as f32 * 3.0).min(1.0)
    }
}

impl Default for ComplexityEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Dynamic Compute Allocator ─────────────────────────────────────────────────

/// Core TTC orchestrator. Evaluates complexity, picks thinking mode,
/// invokes the appropriate execution path, and records the decision to ledger.
pub struct DynamicComputeAllocator {
    estimator: ComplexityEstimator,
    ledger: EventLedger,
    keypair: ZaionKeypair,
    ns_key: NamespaceKey,
}

impl DynamicComputeAllocator {
    pub fn new(
        estimator: ComplexityEstimator,
        ledger: EventLedger,
        keypair: ZaionKeypair,
        ns_key: NamespaceKey,
    ) -> Self {
        DynamicComputeAllocator {
            estimator,
            ledger,
            keypair,
            ns_key,
        }
    }

    /// Allocate and run. Returns (output, mode_used, complexity_score).
    ///
    /// `handler` is called with (task_type, input, token_budget).
    /// For Deep mode the MctsPlanner is used; caller receives the best candidate.
    pub fn run<F>(
        &mut self,
        task_type: &str,
        input: &str,
        forced_mode: Option<ThinkingMode>,
        handler: F,
    ) -> Result<TtcResult, RuntimeError>
    where
        F: Fn(&str, &str, usize) -> Result<String, String>,
    {
        let score = self.estimator.estimate(task_type, input, None);
        let mode = score.to_mode(forced_mode);

        let output = match mode {
            ThinkingMode::Fast => {
                handler(task_type, input, mode.token_budget()).map_err(RuntimeError::TaskFailed)?
            }
            ThinkingMode::Deep {
                max_paths,
                token_budget,
            } => {
                // Use MctsPlanner to generate candidate paths
                use crate::mcts::MctsPlanner;
                let planner = MctsPlanner::new(max_paths, token_budget);
                let candidates = planner.generate_candidates(task_type, input, &handler)?;
                planner.select_best(candidates)
            }
        };

        // Log compute allocation decision to ledger
        let payload = serde_json::json!({
            "task_type": task_type,
            "mode": format!("{:?}", mode),
            "complexity_score": score.score,
            "complexity_factors": score.factors,
        });
        let _ = self.ledger.append_signed_event(
            &self.keypair,
            &self.ns_key,
            "ttc.compute_allocated",
            payload,
            None,
        );

        Ok(TtcResult {
            output,
            mode_used: mode,
            complexity: score,
        })
    }
}

/// Outcome of a TTC-allocated task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtcResult {
    pub output: String,
    pub mode_used: ThinkingMode,
    pub complexity: ComplexityScore,
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn estimator() -> ComplexityEstimator {
        ComplexityEstimator::new()
    }

    #[test]
    fn simple_greeting_is_fast() {
        let s = estimator().estimate("chat", "hello", None);
        assert!(
            s.score < ComplexityScore::FAST_THRESHOLD,
            "score={}",
            s.score
        );
        assert_eq!(s.to_mode(None), ThinkingMode::Fast);
    }

    #[test]
    fn architecture_task_is_deep() {
        let s = estimator().estimate(
            "architecture",
            "design a distributed system with trade-off analysis and multi-region strategy",
            None,
        );
        assert!(
            s.score >= ComplexityScore::FAST_THRESHOLD,
            "score={}",
            s.score
        );
        assert!(s.to_mode(None).is_deep());
    }

    #[test]
    fn forced_mode_overrides_estimator() {
        let s = estimator().estimate("chat", "hi", None);
        let forced = ThinkingMode::Deep {
            max_paths: 2,
            token_budget: 4_000,
        };
        assert_eq!(s.to_mode(Some(forced)), forced);
    }

    #[test]
    fn factors_are_present() {
        let s = estimator().estimate("code", "refactor this function", Some(8_000));
        assert!(s.factors.contains_key("input_length"));
        assert!(s.factors.contains_key("task_type"));
        assert!(s.factors.contains_key("explicit_budget"));
    }
}
