use serde::{Deserialize, Serialize};
use thiserror::Error;
use zaion_types::envelope::CanonicalEnvelope;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteIdentityProof {
    pub proof_type: String,
    pub proof_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityClaims {
    pub capability_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustChainProof {
    pub verifier: String,
    pub chain_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FederationQuota {
    pub max_turns: u32,
    pub max_tool_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationMessage {
    pub envelope: CanonicalEnvelope,
    pub source: String,
    pub remote_principal: String,
    pub remote_identity_proof: RemoteIdentityProof,
    pub remote_capability_claims: CapabilityClaims,
    pub trust_chain: TrustChainProof,
    pub quota: FederationQuota,
}

#[derive(Debug, Error)]
pub enum FederationMessageError {
    #[error("remote principal must use zaion: prefix")]
    InvalidRemotePrincipal,
    #[error("remote identity proof is missing")]
    MissingIdentityProof,
}

impl FederationMessage {
    pub fn new(
        envelope: CanonicalEnvelope,
        remote_principal: impl Into<String>,
        remote_identity_proof: RemoteIdentityProof,
        trust_chain: TrustChainProof,
        quota: FederationQuota,
    ) -> Self {
        Self {
            envelope,
            source: "remote".to_string(),
            remote_principal: remote_principal.into(),
            remote_identity_proof,
            remote_capability_claims: CapabilityClaims {
                capability_ids: Vec::new(),
            },
            trust_chain,
            quota,
        }
    }

    pub fn verify_shape(&self) -> Result<(), FederationMessageError> {
        if !self.remote_principal.starts_with("zaion:") {
            return Err(FederationMessageError::InvalidRemotePrincipal);
        }
        if self.remote_identity_proof.proof_hash.is_empty() {
            return Err(FederationMessageError::MissingIdentityProof);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_types::envelope::CanonicalEnvelope;
    use zaion_types::identity::PrincipalId;
    use zaion_types::session::{ChannelId, ThreadId};

    #[test]
    fn remote_message_requires_remote_principal_and_identity_proof() {
        let envelope = CanonicalEnvelope::new(
            "federation",
            PrincipalId("zaion:remote-peer".to_string()),
            ChannelId("federation".to_string()),
            ThreadId("peer-thread".to_string()),
            "remote-message-1",
            "hello",
            None,
        )
        .expect("canonical remote envelope");
        let message = FederationMessage::new(
            envelope,
            "zaion:remote-peer",
            RemoteIdentityProof {
                proof_type: "signed_agent_card".to_string(),
                proof_hash: "sha256:proof".to_string(),
            },
            TrustChainProof {
                verifier: "self".to_string(),
                chain_hash: "sha256:chain".to_string(),
            },
            FederationQuota {
                max_turns: 1,
                max_tool_calls: 0,
            },
        );

        assert!(message.verify_shape().is_ok());
        assert_eq!(message.remote_principal, "zaion:remote-peer");
    }
}
