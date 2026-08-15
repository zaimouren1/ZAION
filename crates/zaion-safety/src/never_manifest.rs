use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeverCheckRequest {
    pub action: String,
    pub target: String,
    pub payload_preview: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NeverEffect {
    Allow,
    DenyAndQuarantine,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NeverDecision {
    pub effect: NeverEffect,
    pub reason_code: &'static str,
    pub escalation_level: u8,
}

pub fn never_check(request: &NeverCheckRequest) -> NeverDecision {
    let text = format!("{} {}", request.action, request.target).to_ascii_lowercase();
    let forbidden = [
        "modify ledger integrity",
        "overwrite identity key",
        "disable doctor",
        "forge channel.received",
        "anonymous tool receipt",
        "impersonate principal",
        "fake zaion signature",
    ];
    if forbidden.iter().any(|needle| text.contains(needle)) {
        return NeverDecision {
            effect: NeverEffect::DenyAndQuarantine,
            reason_code: "never_manifest_forbidden_action",
            escalation_level: 3,
        };
    }
    NeverDecision {
        effect: NeverEffect::Allow,
        reason_code: "not_forbidden",
        escalation_level: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_manifest_blocks_ledger_integrity_mutation() {
        let decision = never_check(&NeverCheckRequest {
            action: "modify ledger integrity verification code".to_string(),
            target: "zaion-ledger".to_string(),
            payload_preview: serde_json::json!({}),
        });
        assert_eq!(decision.effect, NeverEffect::DenyAndQuarantine);
        assert_eq!(decision.escalation_level, 3);
    }
}
