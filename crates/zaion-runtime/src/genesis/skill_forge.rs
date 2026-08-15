/// Genesis Protocol — SkillForge (Self-Evolution Engine)
///
/// SkillForge distills new skills from task outcomes.
/// It wraps SkillStore with forge/temper semantics.
///
/// Pipeline:
///   task outcome → raw pattern → SkillForge.forge() → SkillStore.upsert
///   → SkillForge.temper() → reinforce via upsert delta
use serde::{Deserialize, Serialize};
use zaion_memory::{MemoryError, SkillStore};
use zaion_types::identity::PrincipalId;

// ── ForgeInput ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeInput {
    pub principal_id: String,
    pub task_type: String,
    pub outcome: ForgeOutcome,
    pub raw_pattern: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ForgeOutcome {
    Success,
    Failure { reason: String },
    Partial,
}

// ── ForgeResult ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeResult {
    pub skill_id: String,
    pub action: ForgeAction,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ForgeAction {
    Created,
    Reinforced,
    Rejected { reason: String },
}

// ── SkillForge ────────────────────────────────────────────────────────────────

pub struct SkillForge {
    store: SkillStore,
    min_pattern_len: usize,
}

impl SkillForge {
    pub fn new(store: SkillStore) -> Self {
        SkillForge {
            store,
            min_pattern_len: 10,
        }
    }

    pub fn forge(&self, input: ForgeInput) -> Result<ForgeResult, MemoryError> {
        let pattern = input.raw_pattern.trim().to_string();

        if pattern.len() < self.min_pattern_len {
            return Ok(ForgeResult {
                skill_id: String::new(),
                action: ForgeAction::Rejected {
                    reason: format!("pattern too short ({} chars)", pattern.len()),
                },
                confidence: 0.0,
            });
        }

        let skill_type = match &input.outcome {
            ForgeOutcome::Success => format!("positive.{}", input.task_type),
            ForgeOutcome::Failure { .. } => format!("avoidance.{}", input.task_type),
            ForgeOutcome::Partial => format!("heuristic.{}", input.task_type),
        };

        let confidence_delta = match &input.outcome {
            ForgeOutcome::Success => 1.0,
            ForgeOutcome::Failure { .. } => 0.9,
            ForgeOutcome::Partial => 0.6,
        };

        let tags: Vec<&str> = input.tags.iter().map(|s| s.as_str()).collect();
        let pid = PrincipalId(input.principal_id.clone());

        let skill_id = self
            .store
            .upsert(&pid, &skill_type, &tags, &pattern, confidence_delta)?;

        // Check if existing (usage_count > 0 means it existed before)
        let entry = self.store.get(&skill_id)?;
        let action = match entry {
            Some(e) if e.usage_count > 0 => ForgeAction::Reinforced,
            _ => ForgeAction::Created,
        };

        Ok(ForgeResult {
            skill_id,
            action,
            confidence: confidence_delta,
        })
    }

    /// Reinforce (positive) or weaken (negative) a skill by upsert delta.
    pub fn temper(
        &self,
        principal_id: &str,
        skill_id: &str,
        positive_uses: usize,
        negative_uses: usize,
    ) -> Result<f64, MemoryError> {
        let delta = positive_uses as f64 * 0.02 - negative_uses as f64 * 0.05;
        if delta.abs() < 1e-9 {
            return Ok(0.0);
        }

        // Fetch current entry to get skill_type / rule_text for upsert
        let entry = self.store.get(skill_id)?;
        if let Some(e) = entry {
            let pid = PrincipalId(principal_id.to_string());
            let tags: Vec<&str> = e.context_tags.iter().map(|s| s.as_str()).collect();
            self.store
                .upsert(&pid, &e.skill_type, &tags, &e.rule_text, delta)?;
        }
        Ok(delta)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_memory::SkillStore;

    fn temp_store() -> SkillStore {
        let path =
            std::env::temp_dir().join(format!("zaion_forge_test_{}.db", uuid::Uuid::new_v4()));
        SkillStore::new(path)
    }

    fn make_input(task_type: &str, pattern: &str, outcome: ForgeOutcome) -> ForgeInput {
        ForgeInput {
            principal_id: "test_principal".into(),
            task_type: task_type.into(),
            outcome,
            raw_pattern: pattern.into(),
            tags: vec!["test".into()],
        }
    }

    #[test]
    fn forge_creates_skill_on_success() {
        let forge = SkillForge::new(temp_store());
        let result = forge
            .forge(make_input(
                "code",
                "Always write unit tests before implementation to catch regressions early",
                ForgeOutcome::Success,
            ))
            .unwrap();
        assert!(!matches!(result.action, ForgeAction::Rejected { .. }));
        assert!(!result.skill_id.is_empty());
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn forge_rejects_too_short_pattern() {
        let forge = SkillForge::new(temp_store());
        let result = forge
            .forge(make_input("code", "short", ForgeOutcome::Success))
            .unwrap();
        assert!(matches!(result.action, ForgeAction::Rejected { .. }));
    }

    #[test]
    fn forge_avoidance_rule_on_failure() {
        let forge = SkillForge::new(temp_store());
        let result = forge
            .forge(make_input(
                "code",
                "Never skip input validation at API boundaries — caused prod outage",
                ForgeOutcome::Failure {
                    reason: "null pointer".into(),
                },
            ))
            .unwrap();
        assert!(!matches!(result.action, ForgeAction::Rejected { .. }));
        assert!(result.confidence >= 0.9);
    }

    #[test]
    fn temper_returns_correct_delta() {
        let forge = SkillForge::new(temp_store());
        let create_result = forge
            .forge(make_input(
                "design",
                "Use event sourcing for audit-critical data paths, not CRUD",
                ForgeOutcome::Success,
            ))
            .unwrap();
        let delta = forge
            .temper("test_principal", &create_result.skill_id, 5, 1)
            .unwrap();
        // 5 * 0.02 - 1 * 0.05 = 0.05
        assert!((delta - 0.05).abs() < 1e-9);
    }
}
