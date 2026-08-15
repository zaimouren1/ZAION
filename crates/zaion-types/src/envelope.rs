use crate::identity::{PrincipalId, SignatureBytes};
use crate::session::{ChannelId, ThreadId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanonicalEnvelopeError {
    #[error("canonical envelope source is empty")]
    EmptySource,
    #[error("canonical envelope principal is empty")]
    EmptyPrincipal,
    #[error("canonical envelope principal is not production-safe: {0}")]
    UnsafePrincipal(String),
    #[error("canonical envelope channel is empty")]
    EmptyChannel,
    #[error("canonical envelope thread is empty")]
    EmptyThread,
    #[error("canonical envelope message_id is empty")]
    EmptyMessageId,
    #[error("canonical envelope source_hash is empty")]
    EmptySourceHash,
    #[error("canonical envelope source_hash must be 64 lowercase hex chars")]
    InvalidSourceHash,
    #[error("canonical envelope source_hash does not match the envelope body")]
    SourceHashMismatch,
    #[error("canonical envelope body is empty")]
    EmptyBody,
}

/// The one ingress envelope shared by CLI, TUI, Telegram, HTTP, MCP and future adapters.
///
/// Runtime code must append `channel.received` from this type instead of
/// hand-building user-input payloads. The signature is optional for now because
/// several channels cannot yet cryptographically bind their transport payloads,
/// but the field is present so later channel binding can be added without
/// changing the persisted shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalEnvelope {
    pub schema_version: u8,
    pub source: String,
    pub principal: PrincipalId,
    pub channel: ChannelId,
    pub thread: ThreadId,
    pub message_id: String,
    pub source_hash: String,
    pub body: String,
    pub received_at: String,
    pub signature: Option<SignatureBytes>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl CanonicalEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: impl Into<String>,
        principal: PrincipalId,
        channel: ChannelId,
        thread: ThreadId,
        message_id: impl Into<String>,
        body: impl Into<String>,
        source_hash: Option<String>,
    ) -> Result<Self, CanonicalEnvelopeError> {
        let source = source.into();
        let message_id = message_id.into();
        let body = body.into();
        let source_hash = source_hash.unwrap_or_else(|| {
            compute_source_hash(
                &source,
                principal.as_str(),
                &channel.0,
                &thread.0,
                &message_id,
                &body,
            )
        });

        let envelope = Self {
            schema_version: 1,
            source,
            principal,
            channel,
            thread,
            message_id,
            source_hash,
            body,
            received_at: chrono::Utc::now().to_rfc3339(),
            signature: None,
            metadata: BTreeMap::new(),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), CanonicalEnvelopeError> {
        if self.source.trim().is_empty() {
            return Err(CanonicalEnvelopeError::EmptySource);
        }
        if self.principal.as_str().trim().is_empty() {
            return Err(CanonicalEnvelopeError::EmptyPrincipal);
        }
        if is_unsafe_principal(self.principal.as_str()) {
            return Err(CanonicalEnvelopeError::UnsafePrincipal(
                self.principal.as_str().to_string(),
            ));
        }
        if self.channel.0.trim().is_empty() {
            return Err(CanonicalEnvelopeError::EmptyChannel);
        }
        if self.thread.0.trim().is_empty() {
            return Err(CanonicalEnvelopeError::EmptyThread);
        }
        if self.message_id.trim().is_empty() {
            return Err(CanonicalEnvelopeError::EmptyMessageId);
        }
        if self.source_hash.trim().is_empty() {
            return Err(CanonicalEnvelopeError::EmptySourceHash);
        }
        if !is_sha256_hex(&self.source_hash) {
            return Err(CanonicalEnvelopeError::InvalidSourceHash);
        }
        let expected = compute_source_hash(
            &self.source,
            self.principal.as_str(),
            &self.channel.0,
            &self.thread.0,
            &self.message_id,
            &self.body,
        );
        if self.source_hash != expected {
            return Err(CanonicalEnvelopeError::SourceHashMismatch);
        }
        if self.body.trim().is_empty() {
            return Err(CanonicalEnvelopeError::EmptyBody);
        }
        Ok(())
    }

    pub fn session_id(&self) -> String {
        format!("{}:{}", self.principal.as_str(), self.thread.0)
    }

    pub fn envelope_id(&self) -> String {
        self.source_hash[..16].to_string()
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn to_channel_received_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": "zaion.canonical_envelope.v1",
            "envelope_id": self.envelope_id(),
            "source": self.source,
            "principal_id": self.principal.as_str(),
            "channel_id": self.channel.0,
            "thread_id": self.thread.0,
            "message_id": self.message_id,
            "source_message_id": self.message_id,
            "source_hash": self.source_hash,
            "received_at": self.received_at,
            "signature_present": self.signature.is_some(),
            "metadata": self.metadata,
            "content": self.body,
            "message": self.body,
        })
    }
}

pub fn compute_source_hash(
    source: &str,
    principal: &str,
    channel: &str,
    thread: &str,
    message_id: &str,
    body: &str,
) -> String {
    let mut hasher = Sha256::new();
    for part in [source, principal, channel, thread, message_id, body] {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    hex::encode(hasher.finalize())
}

/// Validate and normalize the single runtime ingress envelope.
///
/// Callers that receive external input should build a [`CanonicalEnvelope`]
/// at the transport edge, then pass it through this function before runtime
/// dispatch. Keeping this as a named step makes ingress bypasses easy to spot
/// in source review and doctor gates.
pub fn ingest(envelope: &CanonicalEnvelope) -> Result<CanonicalEnvelope, CanonicalEnvelopeError> {
    envelope.validate()?;
    Ok(envelope.clone())
}

pub fn is_unsafe_principal(principal: &str) -> bool {
    let normalized = principal.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "default"
            | "default_principal"
            | "unbound"
            | "unbound-principal"
            | "anonymous"
            | "ephemeral"
            | "principal_placeholder"
    )
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value == value.to_ascii_lowercase()
}
