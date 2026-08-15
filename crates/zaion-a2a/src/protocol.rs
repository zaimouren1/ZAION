use crate::{agent_card::AgentCard, A2AError};
use serde::{Deserialize, Serialize};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::identity::SignatureBytes;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Delegate,
    Result,
    Heartbeat,
    CardExchange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AMessage {
    pub message_id: String,
    pub from_principal: String,
    pub to_principal: String,
    pub message_type: MessageType,
    pub payload: serde_json::Value,
    pub created_at: String,
    pub signature_hex: String,
}

impl A2AMessage {
    pub fn new(
        keypair: &ZaionKeypair,
        to_principal: &str,
        message_type: MessageType,
        payload: serde_json::Value,
    ) -> Self {
        let from_principal = keypair.principal_id().as_str().to_string();
        let message_id = format!("msg-{}", uuid::Uuid::new_v4());
        let created_at = chrono::Utc::now().to_rfc3339();
        let content = format!("{}:{}:{}", message_id, to_principal, payload);
        let sig = keypair.sign(content.as_bytes());
        Self {
            message_id,
            from_principal,
            to_principal: to_principal.to_string(),
            message_type,
            payload,
            created_at,
            signature_hex: hex::encode(&sig.0),
        }
    }

    pub fn verify(&self, sender_card: &AgentCard) -> Result<(), A2AError> {
        let pub_key_bytes = hex::decode(&sender_card.public_key_hex)
            .map_err(|e| A2AError::AuthFailed(e.to_string()))?;
        let pub_key = zaion_types::identity::PublicKeyBytes(pub_key_bytes);
        let content = format!("{}:{}:{}", self.message_id, self.to_principal, self.payload);
        let sig_bytes =
            hex::decode(&self.signature_hex).map_err(|e| A2AError::AuthFailed(e.to_string()))?;
        let sig = SignatureBytes(sig_bytes);
        zaion_crypto::verify::verify_signature(&pub_key, content.as_bytes(), &sig)
            .map_err(|e| A2AError::AuthFailed(e.to_string()))
    }
}
