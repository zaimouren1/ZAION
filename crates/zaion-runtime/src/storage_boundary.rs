use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageBoundaryError {
    #[error("append-only event write failed: {0}")]
    EventAppend(String),
    #[error("knowledge write missing proof event id")]
    MissingLedgerEventId,
    #[error("session write must remain ttl-bound")]
    MissingTtl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventAppend {
    pub event_type: String,
    pub payload: Value,
    pub parent_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeWrite {
    pub collection: String,
    pub payload: Value,
    pub ledger_event_id: String,
}

impl KnowledgeWrite {
    pub fn new(
        collection: impl Into<String>,
        payload: Value,
        ledger_event_id: impl Into<String>,
    ) -> Self {
        Self {
            collection: collection.into(),
            payload,
            ledger_event_id: ledger_event_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionWrite {
    pub key: String,
    pub payload: Value,
    pub ttl_seconds: u64,
    pub proof_persistent: bool,
}

impl SessionWrite {
    pub fn new(key: impl Into<String>, payload: Value, ttl_seconds: u64) -> Self {
        Self {
            key: key.into(),
            payload,
            ttl_seconds: ttl_seconds.max(1),
            proof_persistent: false,
        }
    }
}

pub trait EventStore {
    fn append_only(&self, append: EventAppend) -> Result<String, StorageBoundaryError>;
}

pub trait KnowledgeStore {
    fn write_with_event(&self, write: KnowledgeWrite) -> Result<String, StorageBoundaryError>;
}

pub trait SessionStore {
    fn write_ttl(&self, write: SessionWrite) -> Result<(), StorageBoundaryError>;
    fn remove_expired(&self, now_epoch_seconds: u64) -> Result<usize, StorageBoundaryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_write_requires_ledger_event_id() {
        let write = KnowledgeWrite::new(
            "memory.atom",
            serde_json::json!({"text": "source-backed memory"}),
            "evt-1",
        );
        assert_eq!(write.ledger_event_id, "evt-1");
    }

    #[test]
    fn session_write_records_ttl_and_is_not_proof_state() {
        let write = SessionWrite::new(
            "context-pack-cache",
            serde_json::json!({"context_pack_id": "ctx-1"}),
            600,
        );
        assert_eq!(write.ttl_seconds, 600);
        assert!(!write.proof_persistent);
    }
}
