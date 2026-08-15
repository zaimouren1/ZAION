use crate::RuntimeError;
/// MCTS — Monte Carlo Tree Search Planner (Patch TTC, Godkiller Blueprint)
///
/// Implements multi-path candidate generation for System-2 deep thinking.
/// For each task we generate N independent reasoning paths (candidates),
/// then select the best using a scoring heuristic.
///
/// Design:
///   MctsPlanner::generate_candidates() → Vec<Candidate>
///   MctsPlanner::select_best(candidates) → String (best output)
///
/// The handler closure (same signature as TTC handler) is called once per path.
/// In a real deployment each call would use a different temperature / seed.
use serde::{Deserialize, Serialize};

// ── Candidate ────────────────────────────────────────────────────────────────

/// A single reasoning path produced by one MCTS rollout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    /// Path index (0-based)
    pub path_id: usize,
    /// Raw output from the handler
    pub output: String,
    /// Heuristic quality score (0.0–1.0)
    pub score: f32,
    /// Metadata for audit
    pub token_budget: usize,
}

impl Candidate {
    fn new(path_id: usize, output: String, token_budget: usize) -> Self {
        let score = Self::heuristic_score(&output);
        Candidate {
            path_id,
            output,
            score,
            token_budget,
        }
    }

    /// Simple heuristic: longer, structured outputs score higher (up to a cap).
    /// Real implementation would use a verifier / critic model.
    fn heuristic_score(output: &str) -> f32 {
        let len_score = (output.len() as f32 / 2_000.0).min(1.0);

        // Reward structure markers
        let structure_bonus: f32 = [
            output.contains('\n'),
            output.contains("1.") || output.contains("- "),
            output.contains("because") || output.contains("therefore"),
            output.contains("step") || output.contains("first"),
        ]
        .iter()
        .filter(|&&b| b)
        .count() as f32
            * 0.05;

        // Penalise very short or error-like outputs
        let penalty = if output.len() < 20 || output.to_lowercase().contains("error") {
            0.3
        } else {
            0.0
        };

        (0.6 * len_score + 0.4 * structure_bonus - penalty).clamp(0.0, 1.0)
    }
}

// ── MctsPlanner ───────────────────────────────────────────────────────────────

/// Multi-path planner for System-2 deep thinking.
///
/// `max_paths`    — number of independent reasoning paths to generate
/// `token_budget` — per-path token budget passed to the handler
pub struct MctsPlanner {
    pub max_paths: usize,
    pub token_budget: usize,
}

impl MctsPlanner {
    pub fn new(max_paths: usize, token_budget: usize) -> Self {
        MctsPlanner {
            max_paths,
            token_budget,
        }
    }

    /// Generate N candidate paths by invoking the handler N times.
    ///
    /// Handler signature: `(task_type, input, token_budget) → Result<String, String>`
    /// Each path receives the same input but different budget slices,
    /// simulating temperature variation (budget: budget/2, budget, budget*1.5 clipped).
    pub fn generate_candidates<F>(
        &self,
        task_type: &str,
        input: &str,
        handler: &F,
    ) -> Result<Vec<Candidate>, RuntimeError>
    where
        F: Fn(&str, &str, usize) -> Result<String, String>,
    {
        let mut candidates = Vec::with_capacity(self.max_paths);

        for path_id in 0..self.max_paths {
            // Vary effective budget per path to simulate diverse exploration
            let path_budget = match path_id % 3 {
                0 => self.token_budget / 2,
                1 => self.token_budget,
                _ => (self.token_budget * 3 / 2).min(32_000),
            };

            let output = handler(task_type, input, path_budget)
                .map_err(|e| RuntimeError::TaskFailed(format!("mcts path {path_id}: {e}")))?;

            candidates.push(Candidate::new(path_id, output, path_budget));
        }

        Ok(candidates)
    }

    /// Select the best candidate by heuristic score.
    /// Tie-break: longest output wins.
    pub fn select_best(&self, candidates: Vec<Candidate>) -> String {
        candidates
            .into_iter()
            .max_by(|a, b| {
                a.score
                    .partial_cmp(&b.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.output.len().cmp(&b.output.len()))
            })
            .map(|c| c.output)
            .unwrap_or_default()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_handler(task_type: &str, input: &str, budget: usize) -> Result<String, String> {
        Ok(format!(
            "Path result for task={task_type} input_len={} budget={budget}\n\
             Step 1: analyse the problem\n\
             Step 2: because of the constraints, therefore we proceed\n\
             - option A\n- option B",
            input.len()
        ))
    }

    #[test]
    fn generates_correct_number_of_candidates() {
        let planner = MctsPlanner::new(3, 8_000);
        let candidates = planner
            .generate_candidates("design", "design a system", &mock_handler)
            .unwrap();
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn candidates_have_valid_scores() {
        let planner = MctsPlanner::new(2, 4_000);
        let candidates = planner
            .generate_candidates("code", "refactor this function", &mock_handler)
            .unwrap();
        for c in &candidates {
            assert!(
                (0.0..=1.0).contains(&c.score),
                "score out of range: {}",
                c.score
            );
        }
    }

    #[test]
    fn select_best_returns_non_empty() {
        let planner = MctsPlanner::new(3, 8_000);
        let candidates = planner
            .generate_candidates("analysis", "analyse trade-offs", &mock_handler)
            .unwrap();
        let best = planner.select_best(candidates);
        assert!(!best.is_empty());
    }

    #[test]
    fn handler_error_propagates() {
        let planner = MctsPlanner::new(2, 1_000);
        let err_handler = |_: &str, _: &str, _: usize| -> Result<String, String> {
            Err("provider unavailable".to_string())
        };
        let result = planner.generate_candidates("code", "x", &err_handler);
        assert!(result.is_err());
    }
}
