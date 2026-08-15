use serde::{Deserialize, Serialize};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::identity::SignatureBytes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub principal_id: String,
    pub public_key_hex: String,
    pub display_name: String,
    pub capabilities: Vec<String>,
    pub endpoints: Vec<AgentEndpoint>,
    pub version: String,
    pub created_at: String,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEndpoint {
    pub protocol: EndpointProtocol,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndpointProtocol {
    Http,
    Local,
}

impl AgentCard {
    pub fn new(
        keypair: &ZaionKeypair,
        display_name: impl Into<String>,
        capabilities: Vec<String>,
        endpoints: Vec<AgentEndpoint>,
    ) -> Self {
        let principal_id = keypair.principal_id();
        let public_key_hex = hex::encode(&keypair.public_key_bytes().0);
        let created_at = chrono::Utc::now().to_rfc3339();
        let version = env!("CARGO_PKG_VERSION").to_string();
        let display_name = display_name.into();
        let unsigned = serde_json::json!({
            "principal_id": principal_id.as_str(),
            "public_key_hex": public_key_hex,
            "display_name": display_name,
            "capabilities": capabilities,
            "version": version,
            "created_at": created_at,
        });
        let sig = keypair.sign(unsigned.to_string().as_bytes());
        Self {
            principal_id: principal_id.as_str().to_string(),
            public_key_hex,
            display_name,
            capabilities,
            endpoints,
            version,
            created_at,
            signature_hex: hex::encode(&sig.0),
        }
    }

    pub fn verify(&self) -> Result<(), crate::A2AError> {
        let pub_key_bytes = hex::decode(&self.public_key_hex)
            .map_err(|e| crate::A2AError::AuthFailed(e.to_string()))?;
        let pub_key = zaion_types::identity::PublicKeyBytes(pub_key_bytes);
        let unsigned = serde_json::json!({
            "principal_id": self.principal_id,
            "public_key_hex": self.public_key_hex,
            "display_name": self.display_name,
            "capabilities": self.capabilities,
            "version": self.version,
            "created_at": self.created_at,
        });
        let sig_bytes = hex::decode(&self.signature_hex)
            .map_err(|e| crate::A2AError::AuthFailed(e.to_string()))?;
        let sig = SignatureBytes(sig_bytes);
        zaion_crypto::verify::verify_signature(&pub_key, unsigned.to_string().as_bytes(), &sig)
            .map_err(|e| crate::A2AError::AuthFailed(e.to_string()))
    }

    pub fn to_json(&self) -> Result<String, crate::A2AError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self, crate::A2AError> {
        Ok(serde_json::from_str(json)?)
    }
}
