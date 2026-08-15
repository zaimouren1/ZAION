use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnomalySignal {
    IdentityHashMismatch { principal_id: String },
    ProofChainBroken { turn_id: String },
    MissingToolReceipt { call_id: String },
    BehaviorBudgetExceeded { turn_id: String },
    NeverManifestHit { action: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EscalationLevel {
    Level1Reject,
    Level2DegradeTurn,
    Level3Quarantine,
    Level4PanicSafeLockdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EscalationResponse {
    pub level: EscalationLevel,
    pub ledger_event_type: &'static str,
    pub allows_tools: bool,
    pub allows_memory_writes: bool,
}

#[derive(Debug, Default)]
pub struct EscalationEngine;

impl EscalationEngine {
    pub fn classify(&self, signal: &AnomalySignal) -> EscalationResponse {
        match signal {
            AnomalySignal::IdentityHashMismatch { .. }
            | AnomalySignal::ProofChainBroken { .. }
            | AnomalySignal::NeverManifestHit { .. } => EscalationResponse {
                level: EscalationLevel::Level3Quarantine,
                ledger_event_type: "system.quarantine",
                allows_tools: false,
                allows_memory_writes: false,
            },
            AnomalySignal::MissingToolReceipt { .. }
            | AnomalySignal::BehaviorBudgetExceeded { .. } => EscalationResponse {
                level: EscalationLevel::Level2DegradeTurn,
                ledger_event_type: "turn.degraded",
                allows_tools: false,
                allows_memory_writes: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_chain_break_escalates_to_quarantine() {
        let signal = AnomalySignal::ProofChainBroken {
            turn_id: "turn-1".to_string(),
        };
        let response = EscalationEngine.classify(&signal);
        assert_eq!(response.level, EscalationLevel::Level3Quarantine);
        assert!(!response.allows_tools);
        assert!(!response.allows_memory_writes);
    }
}
