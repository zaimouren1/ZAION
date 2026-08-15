use crate::identity::PrincipalId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    pub skill_id: String,
    pub principal_id: PrincipalId,
    pub skill_type: SkillType,
    pub context_tags: Vec<String>,
    pub rule_text: String,
    pub confidence: f64,
    pub usage_count: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillType {
    Pattern,
    Avoidance,
    Shortcut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub checkpoint_id: String,
    pub namespace_key: String,
    pub run_id: String,
    pub summary: String,
    pub artifact_ref: String,
    pub created_at: String,
}
