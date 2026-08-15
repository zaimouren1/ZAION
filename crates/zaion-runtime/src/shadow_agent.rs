use crate::RuntimeError;
/// ShadowAgent — Parallel Shadow Executor (Patch TTC, Godkiller Blueprint)
///
/// Implements "shadow execution": run a task simultaneously on multiple execution
/// strategies, compare outputs, and surface the best. Different from MCTS (which
/// varies token budget), ShadowAgent varies the *strategy* (prompt template).
///
/// Architecture:
///   ShadowAgent::new(strategies) → register named prompt strategies
///   ShadowAgent::run(task, input, handler) → Vec<ShadowResult>
///   ShadowAgent::arbitrate(results) → best ShadowResult
///
/// This is the "compare and arbitrate" layer above MCTS.
use serde::{Deserialize, Serialize};

// ── Strategy ─────────────────────────────────────────────────────────────────

/// A named execution strategy with a prompt prefix/suffix injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStrategy {
    pub name: String,
    /// Prefix injected before the task input (e.g. "Think step by step:")
    pub prompt_prefix: String,
    /// Suffix appended to the task input (e.g. "Provide concise answer only.")
    pub prompt_suffix: String,
    /// Weight for arbitration scoring (higher = preferred when equal quality)
    pub weight: f32,
}

impl ExecutionStrategy {
    pub fn new(name: &str, prefix: &str, suffix: &str, weight: f32) -> Self {
        ExecutionStrategy {
            name: name.to_string(),
            prompt_prefix: prefix.to_string(),
            prompt_suffix: suffix.to_string(),
            weight: weight.clamp(0.0, 2.0),
        }
    }

    /// Apply the strategy to wrap the input text.
    pub fn apply(&self, input: &str) -> String {
        if self.prompt_prefix.is_empty() && self.prompt_suffix.is_empty() {
            input.to_string()
        } else {
            format!("{}\n{}\n{}", self.prompt_prefix, input, self.prompt_suffix)
                .trim()
                .to_string()
        }
    }
}

/// Built-in strategy presets
impl ExecutionStrategy {
    pub fn chain_of_thought() -> Self {
        Self::new(
            "chain_of_thought",
            "Think step by step before answering:",
            "",
            1.2,
        )
    }

    pub fn concise() -> Self {
        Self::new(
            "concise",
            "Answer concisely and directly:",
            "Keep the answer under 3 sentences.",
            0.8,
        )
    }

    pub fn devil_advocate() -> Self {
        Self::new(
            "devil_advocate",
            "Consider counterarguments first:",
            "Then provide the strongest conclusion.",
            1.0,
        )
    }
}

// ── ShadowResult ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowResult {
    pub strategy_name: String,
    pub output: String,
    /// Raw quality score (0.0–1.0) before weight adjustment
    pub raw_score: f32,
    /// Final arbitration score (raw_score * strategy.weight)
    pub final_score: f32,
}

impl ShadowResult {
    fn new(strategy: &ExecutionStrategy, output: String) -> Self {
        let raw_score = Self::score(&output);
        let final_score = (raw_score * strategy.weight).clamp(0.0, 1.0);
        ShadowResult {
            strategy_name: strategy.name.clone(),
            output,
            raw_score,
            final_score,
        }
    }

    fn score(output: &str) -> f32 {
        if output.is_empty() {
            return 0.0;
        }

        // Word count (normalised to 200 words = full score)
        let word_count = output.split_whitespace().count();
        let length_score = (word_count as f32 / 200.0).min(1.0);

        // Coherence signals
        let has_structure = output.contains('\n') || output.contains(". ");
        let no_repetition = {
            let words: Vec<&str> = output.split_whitespace().collect();
            let unique = words.iter().collect::<std::collections::HashSet<_>>().len();
            unique as f32 / words.len().max(1) as f32
        };

        0.4 * length_score + 0.2 * (has_structure as u8 as f32) + 0.4 * no_repetition
    }
}

// ── ShadowAgent ───────────────────────────────────────────────────────────────

/// Parallel shadow executor. Runs all strategies against the same task and input.
pub struct ShadowAgent {
    strategies: Vec<ExecutionStrategy>,
}

impl ShadowAgent {
    pub fn new(strategies: Vec<ExecutionStrategy>) -> Self {
        ShadowAgent { strategies }
    }

    /// Default set: CoT + concise + devil's advocate
    pub fn default_strategies() -> Self {
        ShadowAgent::new(vec![
            ExecutionStrategy::chain_of_thought(),
            ExecutionStrategy::concise(),
            ExecutionStrategy::devil_advocate(),
        ])
    }

    /// Run all strategies. Returns one ShadowResult per strategy.
    ///
    /// Handler: `(task_type, wrapped_input, token_budget) → Result<String, String>`
    pub fn run<F>(
        &self,
        task_type: &str,
        input: &str,
        token_budget: usize,
        handler: &F,
    ) -> Result<Vec<ShadowResult>, RuntimeError>
    where
        F: Fn(&str, &str, usize) -> Result<String, String>,
    {
        let mut results = Vec::with_capacity(self.strategies.len());

        for strategy in &self.strategies {
            let wrapped = strategy.apply(input);
            let output = handler(task_type, &wrapped, token_budget)
                .map_err(|e| RuntimeError::TaskFailed(format!("shadow[{}]: {e}", strategy.name)))?;
            results.push(ShadowResult::new(strategy, output));
        }

        Ok(results)
    }

    /// Pick the strategy result with the highest final_score.
    pub fn arbitrate(&self, results: Vec<ShadowResult>) -> Option<ShadowResult> {
        results.into_iter().max_by(|a, b| {
            a.final_score
                .partial_cmp(&b.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_handler(task_type: &str, input: &str, budget: usize) -> Result<String, String> {
        Ok(format!(
            "[{task_type} budget={budget}] {}: processed successfully with clear reasoning.",
            input.chars().take(40).collect::<String>()
        ))
    }

    #[test]
    fn strategy_apply_wraps_input() {
        let s = ExecutionStrategy::chain_of_thought();
        let wrapped = s.apply("solve x=2+2");
        assert!(wrapped.contains("Think step by step"));
        assert!(wrapped.contains("solve x=2+2"));
    }

    #[test]
    fn shadow_agent_runs_all_strategies() {
        let agent = ShadowAgent::default_strategies();
        let results = agent
            .run("code", "write a sort function", 4_000, &mock_handler)
            .unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn arbitrate_picks_highest_score() {
        let agent = ShadowAgent::default_strategies();
        let results = agent
            .run(
                "design",
                "design a caching layer with trade-off analysis",
                8_000,
                &mock_handler,
            )
            .unwrap();
        let best = agent.arbitrate(results.clone()).unwrap();
        for r in &results {
            assert!(best.final_score >= r.final_score - 1e-6);
        }
    }

    #[test]
    fn handler_error_propagates() {
        let agent = ShadowAgent::default_strategies();
        let err =
            |_: &str, _: &str, _: usize| -> Result<String, String> { Err("timeout".to_string()) };
        let result = agent.run("code", "x", 1_000, &err);
        assert!(result.is_err());
    }

    #[test]
    fn score_is_bounded() {
        let s = ShadowResult::new(
            &ExecutionStrategy::concise(),
            "Short answer. Done.".to_string(),
        );
        assert!((0.0..=1.0).contains(&s.raw_score));
        assert!((0.0..=1.0).contains(&s.final_score));
    }
}
