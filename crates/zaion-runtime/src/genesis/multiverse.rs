/// Genesis Protocol — Multiverse (Parallel Reality Evaluator)
///
/// Multiverse runs multiple independent agent "universes" in parallel,
/// each starting from the same state but taking different action paths.
/// The best-performing universe's outcome is selected.
///
/// This implements the "parallel universe exploration" concept from the
/// Godkiller Blueprint — branching execution at decision points.
///
/// Stub implementation: simulates branching without real async execution.
/// Production: spawns Tokio tasks, each with independent AgentLoop + LLM call.
use serde::{Deserialize, Serialize};

// ── Universe ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Universe {
    pub universe_id: usize,
    /// Strategy variant identifier (e.g. "aggressive", "conservative", "exploratory")
    pub strategy: String,
    /// Final output from this universe's execution
    pub output: String,
    /// Quality score (0.0–1.0), higher = better
    pub score: f32,
    /// Steps taken in this universe
    pub steps: usize,
}

impl Universe {
    pub fn new(universe_id: usize, strategy: &str, output: String, steps: usize) -> Self {
        let score = Self::evaluate(&output, steps);
        Universe {
            universe_id,
            strategy: strategy.to_string(),
            output,
            score,
            steps,
        }
    }

    fn evaluate(output: &str, steps: usize) -> f32 {
        if output.is_empty() {
            return 0.0;
        }

        // Reward: quality content
        let content_score = (output.len() as f32 / 1_500.0).min(1.0);

        // Reward: efficiency (fewer steps = more efficient, up to a cap)
        let step_efficiency = (1.0 - (steps as f32 / 20.0).min(1.0)) * 0.3;

        // Reward: structural coherence
        let coherence = if output.contains('\n') || output.contains(". ") {
            0.2
        } else {
            0.0
        };

        (0.5 * content_score + step_efficiency + coherence).clamp(0.0, 1.0)
    }
}

// ── MultiverseConfig ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiverseConfig {
    /// Number of parallel universes to run
    pub num_universes: usize,
    /// Strategy variants to explore
    pub strategies: Vec<String>,
    /// Token budget per universe
    pub token_budget_per_universe: usize,
}

impl Default for MultiverseConfig {
    fn default() -> Self {
        MultiverseConfig {
            num_universes: 3,
            strategies: vec![
                "aggressive".to_string(),
                "conservative".to_string(),
                "exploratory".to_string(),
            ],
            token_budget_per_universe: 8_000,
        }
    }
}

// ── MultiverseResult ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiverseResult {
    pub universes: Vec<Universe>,
    /// Best universe (highest score)
    pub winner: Universe,
    /// Average score across all universes
    pub mean_score: f32,
    /// Score variance (measure of exploration diversity)
    pub score_variance: f32,
}

// ── Multiverse ─────────────────────────────────────────────────────────────────

pub struct Multiverse {
    config: MultiverseConfig,
}

impl Multiverse {
    pub fn new(config: MultiverseConfig) -> Self {
        Multiverse { config }
    }

    pub fn with_defaults() -> Self {
        Multiverse::new(MultiverseConfig::default())
    }

    /// Run N universes using the provided handler, return the best result.
    ///
    /// Handler: `(task_type, input, strategy, token_budget) → Result<(output, steps), String>`
    /// Each universe receives its own strategy identifier.
    pub fn run<F>(
        &self,
        task_type: &str,
        input: &str,
        handler: &F,
    ) -> Result<MultiverseResult, String>
    where
        F: Fn(&str, &str, &str, usize) -> Result<(String, usize), String>,
    {
        let n = self.config.num_universes;
        let mut universes = Vec::with_capacity(n);

        for i in 0..n {
            let strategy = self
                .config
                .strategies
                .get(i)
                .map(|s| s.as_str())
                .unwrap_or("default");

            let (output, steps) = handler(
                task_type,
                input,
                strategy,
                self.config.token_budget_per_universe,
            )
            .map_err(|e| format!("universe {i} [{strategy}]: {e}"))?;

            universes.push(Universe::new(i, strategy, output, steps));
        }

        let winner = universes
            .iter()
            .max_by(|a, b| {
                a.score
                    .partial_cmp(&b.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
            .ok_or_else(|| "no universes generated".to_string())?;

        let mean_score = universes.iter().map(|u| u.score).sum::<f32>() / universes.len() as f32;
        let score_variance = universes
            .iter()
            .map(|u| (u.score - mean_score).powi(2))
            .sum::<f32>()
            / universes.len() as f32;

        Ok(MultiverseResult {
            universes,
            winner,
            mean_score,
            score_variance,
        })
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_handler(
        task_type: &str,
        input: &str,
        strategy: &str,
        budget: usize,
    ) -> Result<(String, usize), String> {
        let steps = match strategy {
            "aggressive" => 3,
            "conservative" => 8,
            _ => 5,
        };
        let output = format!(
            "[{task_type}/{strategy} budget={budget}] Processed: {input}\n\
             Step 1: analyse.\nStep 2: execute.\nStep 3: verify.\n\
             Result: completed with {steps} steps."
        );
        Ok((output, steps))
    }

    #[test]
    fn runs_correct_number_of_universes() {
        let mv = Multiverse::with_defaults();
        let result = mv.run("code", "implement sorting", &mock_handler).unwrap();
        assert_eq!(result.universes.len(), 3);
    }

    #[test]
    fn winner_has_highest_score() {
        let mv = Multiverse::with_defaults();
        let result = mv.run("design", "design a cache", &mock_handler).unwrap();
        for u in &result.universes {
            assert!(result.winner.score >= u.score - 1e-6);
        }
    }

    #[test]
    fn mean_score_is_bounded() {
        let mv = Multiverse::with_defaults();
        let result = mv
            .run("analysis", "analyse trade-offs", &mock_handler)
            .unwrap();
        assert!((0.0..=1.0).contains(&result.mean_score));
    }

    #[test]
    fn score_variance_is_non_negative() {
        let mv = Multiverse::with_defaults();
        let result = mv.run("code", "refactor module", &mock_handler).unwrap();
        assert!(result.score_variance >= 0.0);
    }

    #[test]
    fn handler_error_propagates() {
        let mv = Multiverse::with_defaults();
        let err = |_: &str, _: &str, _: &str, _: usize| -> Result<(String, usize), String> {
            Err("provider down".to_string())
        };
        assert!(mv.run("code", "x", &err).is_err());
    }
}
