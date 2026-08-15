/// Genesis Protocol — DreamEngine (Hypothetical Scenario Simulator)
///
/// DreamEngine simulates hypothetical future states by generating "what-if"
/// scenarios and evaluating their likelihood and impact. Used for:
///   1. Pre-task planning: simulate N futures before committing to a plan
///   2. Risk assessment: identify high-impact failure modes
///   3. Self-improvement: propose new skill candidates proactively
///
/// This is a stub implementation. Production version would use the TTC
/// DynamicComputeAllocator with System-2 deep thinking per scenario.
use serde::{Deserialize, Serialize};

// ── Scenario ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub description: String,
    /// Probability this scenario occurs (0.0–1.0)
    pub probability: f32,
    /// Expected impact magnitude (0.0–1.0)
    pub impact: f32,
    /// "positive" | "negative" | "neutral"
    pub valence: ScenarioValence,
    /// Suggested action to take if this scenario is selected
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScenarioValence {
    Positive,
    Negative,
    Neutral,
}

impl Scenario {
    /// Expected value = probability * impact (positive) or -probability * impact (negative)
    pub fn expected_value(&self) -> f32 {
        let sign = match self.valence {
            ScenarioValence::Positive => 1.0,
            ScenarioValence::Negative => -1.0,
            ScenarioValence::Neutral => 0.0,
        };
        sign * self.probability * self.impact
    }
}

// ── DreamResult ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamResult {
    pub scenarios: Vec<Scenario>,
    /// The scenario with highest expected value
    pub recommended: Option<Scenario>,
    /// Aggregate risk score (sum of negative EVs)
    pub risk_score: f32,
    /// Aggregate opportunity score (sum of positive EVs)
    pub opportunity_score: f32,
}

// ── DreamEngine ───────────────────────────────────────────────────────────────

pub struct DreamEngine {
    /// Maximum number of scenarios to generate per dream session
    pub max_scenarios: usize,
}

impl DreamEngine {
    pub fn new(max_scenarios: usize) -> Self {
        DreamEngine { max_scenarios }
    }

    /// Simulate hypothetical futures for a given task description.
    ///
    /// Stub: generates structured placeholder scenarios based on task_type.
    /// Production: calls TTC System-2 with scenario-generation prompt templates.
    pub fn dream(&self, task_type: &str, task_description: &str) -> DreamResult {
        let scenarios = self.generate_stub_scenarios(task_type, task_description);
        self.evaluate(scenarios)
    }

    fn generate_stub_scenarios(&self, task_type: &str, description: &str) -> Vec<Scenario> {
        let mut scenarios = Vec::new();
        let base_id = uuid::Uuid::new_v4().to_string();

        // Scenario 1: Best case (success path)
        scenarios.push(Scenario {
            id: format!("{base_id}-best"),
            description: format!(
                "[{task_type}] Best case: task completes on first attempt with high quality output"
            ),
            probability: 0.6,
            impact: 0.9,
            valence: ScenarioValence::Positive,
            suggested_action: Some("Proceed with standard approach".to_string()),
        });

        // Scenario 2: Partial success
        scenarios.push(Scenario {
            id: format!("{base_id}-partial"),
            description: format!(
                "[{task_type}] Partial: task completes but requires iteration — {desc}",
                desc = &description[..description.len().min(50)]
            ),
            probability: 0.30,
            impact: 0.5,
            valence: ScenarioValence::Neutral,
            suggested_action: Some("Prepare fallback plan B".to_string()),
        });

        // Scenario 3: Failure / risk
        scenarios.push(Scenario {
            id: format!("{base_id}-fail"),
            description: format!(
                "[{task_type}] Risk: task fails due to missing context or constraint violation"
            ),
            probability: 0.10,
            impact: 0.8,
            valence: ScenarioValence::Negative,
            suggested_action: Some("Gather more context before proceeding".to_string()),
        });

        scenarios.truncate(self.max_scenarios);
        scenarios
    }

    fn evaluate(&self, scenarios: Vec<Scenario>) -> DreamResult {
        let risk_score: f32 = scenarios
            .iter()
            .filter(|s| s.valence == ScenarioValence::Negative)
            .map(|s| s.probability * s.impact)
            .sum();

        let opportunity_score: f32 = scenarios
            .iter()
            .filter(|s| s.valence == ScenarioValence::Positive)
            .map(|s| s.probability * s.impact)
            .sum();

        let recommended = scenarios
            .iter()
            .max_by(|a, b| {
                a.expected_value()
                    .partial_cmp(&b.expected_value())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned();

        DreamResult {
            scenarios,
            recommended,
            risk_score,
            opportunity_score,
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dream_returns_scenarios() {
        let engine = DreamEngine::new(5);
        let result = engine.dream("code", "implement a sorting algorithm");
        assert!(!result.scenarios.is_empty());
        assert!(result.scenarios.len() <= 5);
    }

    #[test]
    fn recommended_has_highest_ev() {
        let engine = DreamEngine::new(5);
        let result = engine.dream("design", "architect a distributed cache");
        let rec = result.recommended.unwrap();
        for s in &result.scenarios {
            assert!(rec.expected_value() >= s.expected_value() - 1e-6);
        }
    }

    #[test]
    fn risk_and_opportunity_scores_are_non_negative() {
        let engine = DreamEngine::new(5);
        let result = engine.dream("analysis", "analyse trade-offs");
        assert!(result.risk_score >= 0.0);
        assert!(result.opportunity_score >= 0.0);
    }

    #[test]
    fn expected_value_negative_for_failure_scenario() {
        let s = Scenario {
            id: "test".into(),
            description: "fail".into(),
            probability: 0.5,
            impact: 0.8,
            valence: ScenarioValence::Negative,
            suggested_action: None,
        };
        assert!(s.expected_value() < 0.0);
    }

    #[test]
    fn max_scenarios_respected() {
        let engine = DreamEngine::new(2);
        let result = engine.dream("code", "write tests");
        assert!(result.scenarios.len() <= 2);
    }
}
