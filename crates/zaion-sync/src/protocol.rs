use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffRequest {
    pub local_principal: String,
    pub remote_principal: String,
    pub local_head: Option<String>,
    pub local_merkle_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeltaProposal {
    pub event_ids: Vec<String>,
    pub event_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidateAndSign {
    pub accepted_event_ids: Vec<String>,
    pub rejected_event_ids: Vec<String>,
    pub validation_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Apply {
    pub appended_event_ids: Vec<String>,
    pub skipped_existing_event_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForkResolution {
    pub parent_event_id: String,
    pub local_head: String,
    pub remote_head: String,
    pub selected_head: String,
    pub selection_rule: String,
    pub resolver_principal: String,
}

impl ForkResolution {
    pub fn ledger_event_type(&self) -> &'static str {
        "fork.resolved"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncProtocol {
    pub diff_request: DiffRequest,
    pub delta_proposal: Option<DeltaProposal>,
    pub validate_and_sign: Option<ValidateAndSign>,
    pub apply: Option<Apply>,
}

impl SyncProtocol {
    pub fn new(local_principal: impl Into<String>, remote_principal: impl Into<String>) -> Self {
        Self {
            diff_request: DiffRequest {
                local_principal: local_principal.into(),
                remote_principal: remote_principal.into(),
                local_head: None,
                local_merkle_root: None,
            },
            delta_proposal: None,
            validate_and_sign: None,
            apply: None,
        }
    }

    pub fn state_names(&self) -> Vec<&'static str> {
        vec!["DiffRequest", "DeltaProposal", "ValidateAndSign", "Apply"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_protocol_order_is_diff_proposal_validate_apply() {
        let protocol = SyncProtocol::new("did:key:local", "zaion:remote");
        assert_eq!(
            protocol.state_names(),
            vec!["DiffRequest", "DeltaProposal", "ValidateAndSign", "Apply"]
        );
    }

    #[test]
    fn fork_resolution_is_append_only_event() {
        let fork = ForkResolution {
            parent_event_id: "evt-parent".to_string(),
            local_head: "evt-local".to_string(),
            remote_head: "evt-remote".to_string(),
            selected_head: "evt-local".to_string(),
            selection_rule: "longest_verified_hash_chain".to_string(),
            resolver_principal: "did:key:local".to_string(),
        };

        assert_eq!(fork.ledger_event_type(), "fork.resolved");
        assert_eq!(fork.selection_rule, "longest_verified_hash_chain");
    }
}
