//! SessionStore trait adapter for zaion-ledger integration
//!
//! This module provides the bridge between zaion-ledger's SessionStore
//! and zaion-runtime's SessionStore trait, enabling /branch command
//! to work with the SQLite-backed session storage.

use crate::{SessionMetadata, SessionStore as SessionStoreTrait};
use zaion_crypto::ZaionKeypair;
use zaion_ledger::{EventLedger, SessionEntry, SessionStore};
use zaion_types::envelope::is_unsafe_principal;
use zaion_types::session::{NamespaceKey, SessionKey};

/// Adapter implementing SessionStore trait for zaion-ledger::SessionStore
pub struct SessionStoreAdapter {
    store: SessionStore,
    ledger: Option<EventLedger>,
    keypair: Option<ZaionKeypair>,
    principal_id: String,
}

impl SessionStoreAdapter {
    pub fn new(store: SessionStore, principal_id: impl Into<String>) -> Result<Self, String> {
        let principal_id = validate_adapter_principal(principal_id)?;
        Ok(Self {
            store,
            ledger: None,
            keypair: None,
            principal_id,
        })
    }

    pub fn new_with_ledger(
        store: SessionStore,
        ledger: EventLedger,
        keypair: ZaionKeypair,
    ) -> Result<Self, String> {
        let principal_id = validate_adapter_principal(keypair.principal_id().0.clone())?;
        Ok(Self {
            store,
            ledger: Some(ledger),
            keypair: Some(keypair),
            principal_id,
        })
    }
}

impl SessionStoreTrait for SessionStoreAdapter {
    fn get_session(&self, session_id: &str) -> Result<Option<SessionMetadata>, String> {
        self.store
            .get_session(session_id)
            .map(|opt| {
                opt.map(|entry| SessionMetadata {
                    principal_id: entry.principal_id,
                    session_id: entry.session_id,
                    parent_session_id: entry.parent_session_id,
                    title: Some(entry.session_key),
                    model: "unknown".to_string(),
                    source: entry.platform,
                    chat_id: Some(entry.chat_id),
                    user_id: entry.user_id,
                    thread_id: entry.thread_id,
                    end_reason: entry.end_reason,
                    created_at: entry.created_at,
                    ended_at: None,
                })
            })
            .map_err(|e| e.to_string())
    }

    fn create_session(&self, metadata: SessionMetadata) -> Result<(), String> {
        let entry = SessionEntry {
            session_id: metadata.session_id.clone(),
            principal_id: self.principal_for_metadata(&metadata)?,
            platform: metadata.source.clone(),
            chat_id: metadata
                .chat_id
                .clone()
                .unwrap_or_else(|| "default".to_string()),
            user_id: metadata.user_id.clone(),
            thread_id: metadata.thread_id.clone(),
            session_key: metadata
                .title
                .clone()
                .unwrap_or_else(|| metadata.session_id.clone()),
            created_at: metadata.created_at.clone(),
            updated_at: metadata.created_at.clone(),
            message_count: 0,
            tool_call_count: 0,
            estimated_cost_usd: 0.0,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: metadata.parent_session_id,
            end_reason: metadata.end_reason,
        };
        self.store.upsert_session(&entry).map_err(|e| e.to_string())
    }

    fn update_session(&self, session_id: &str, metadata: SessionMetadata) -> Result<(), String> {
        let entry = SessionEntry {
            session_id: session_id.to_string(),
            principal_id: self.principal_for_metadata(&metadata)?,
            platform: metadata.source.clone(),
            chat_id: metadata
                .chat_id
                .clone()
                .unwrap_or_else(|| "default".to_string()),
            user_id: metadata.user_id.clone(),
            thread_id: metadata.thread_id.clone(),
            session_key: metadata
                .title
                .clone()
                .unwrap_or_else(|| session_id.to_string()),
            created_at: metadata.created_at.clone(),
            updated_at: metadata.created_at.clone(),
            message_count: 0,
            tool_call_count: 0,
            estimated_cost_usd: 0.0,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: metadata.parent_session_id,
            end_reason: metadata.end_reason,
        };
        self.store.upsert_session(&entry).map_err(|e| e.to_string())
    }

    fn get_title(&self, session_id: &str) -> Result<Option<String>, String> {
        self.store.get_title(session_id).map_err(|e| e.to_string())
    }

    fn set_title(&self, session_id: &str, title: &str) -> Result<(), String> {
        self.store
            .set_title(session_id, title)
            .map_err(|e| e.to_string())
    }

    fn copy_history(&self, from_session: &str, to_session: &str) -> Result<usize, String> {
        let ledger = self.ledger.as_ref().ok_or_else(|| {
            "SessionStoreAdapter::copy_history requires EventLedger for proof-preserving session history copy"
                .to_string()
        })?;
        let keypair = self.keypair.as_ref().ok_or_else(|| {
            "SessionStoreAdapter::copy_history requires EventLedger and persisted ZaionKeypair"
                .to_string()
        })?;
        if from_session.trim().is_empty() || to_session.trim().is_empty() {
            return Err("copy_history requires non-empty source and target session ids".into());
        }

        let mut source_events = ledger
            .list_events(&SessionKey(from_session.to_string()), None, 10_000)
            .map_err(|error| error.to_string())?;
        source_events.reverse();

        let target_namespace = NamespaceKey(to_session.to_string());
        let mut copied = 0usize;

        for source in source_events {
            let payload = serde_json::json!({
                "schema": "zaion.session_history_copy.v1",
                "from_session": from_session,
                "to_session": to_session,
                "source_event_id": source.event_id.0,
                "source_event_type": source.event_type,
                "source_namespace_key": source.namespace_key.0,
                "source_run_id": source.run_id.as_ref().map(|run| run.0.clone()),
                "source_parent_event_id": source.parent_event_id.as_ref().map(|event| event.0.clone()),
                "source_created_at": source.created_at,
                "source_signature_present": source.signature.is_some(),
                "source_payload": source.payload,
                "copy_policy": "lineage_pointer",
            });

            ledger
                .append_signed_event_with_parent(
                    keypair,
                    &target_namespace,
                    "session.history.copied",
                    payload,
                    source.run_id.as_ref(),
                    Some(&source.event_id),
                )
                .map_err(|error| error.to_string())?;
            copied += 1;
        }

        Ok(copied)
    }
}

impl SessionStoreAdapter {
    fn principal_for_metadata(&self, metadata: &SessionMetadata) -> Result<String, String> {
        let principal = if metadata.principal_id.trim().is_empty() {
            self.principal_id.clone()
        } else {
            metadata.principal_id.clone()
        };
        if is_unsafe_principal(&principal) {
            return Err(format!(
                "session metadata principal is not production-safe: {}",
                principal
            ));
        }
        Ok(principal)
    }
}

fn validate_adapter_principal(principal_id: impl Into<String>) -> Result<String, String> {
    let principal_id = principal_id.into();
    if is_unsafe_principal(&principal_id) {
        return Err(format!(
            "SessionStoreAdapter requires a production-safe principal, got {}",
            principal_id
        ));
    }
    Ok(principal_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_adapter_get_session() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let adapter = SessionStoreAdapter::new(store, "principal-adapter-test").unwrap();

        // Non-existent session returns None
        let result = adapter.get_session("nonexistent");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_adapter_create_and_get_session() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let adapter = SessionStoreAdapter::new(store, "principal-adapter-test").unwrap();

        let metadata = SessionMetadata {
            principal_id: "principal-adapter-test".to_string(),
            session_id: "test-session".to_string(),
            parent_session_id: None,
            title: Some("Test Session".to_string()),
            model: "gpt-4".to_string(),
            source: "terminal".to_string(),
            chat_id: Some("chat-1".to_string()),
            user_id: Some("user-1".to_string()),
            thread_id: Some("thread-1".to_string()),
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        };

        adapter.create_session(metadata.clone()).unwrap();

        let retrieved = adapter.get_session("test-session").unwrap().unwrap();
        assert_eq!(retrieved.session_id, "test-session");
        assert_eq!(retrieved.title, Some("Test Session".to_string()));
        assert_eq!(retrieved.chat_id.as_deref(), Some("chat-1"));
        assert_eq!(retrieved.user_id.as_deref(), Some("user-1"));
        assert_eq!(retrieved.thread_id.as_deref(), Some("thread-1"));
    }

    #[test]
    fn test_adapter_update_session() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let adapter = SessionStoreAdapter::new(store, "principal-adapter-test").unwrap();

        let metadata = SessionMetadata {
            principal_id: "principal-adapter-test".to_string(),
            session_id: "test-session".to_string(),
            parent_session_id: None,
            title: Some("Original Title".to_string()),
            model: "gpt-4".to_string(),
            source: "terminal".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        };

        adapter.create_session(metadata.clone()).unwrap();

        let updated_metadata = SessionMetadata {
            principal_id: "principal-adapter-test".to_string(),
            session_id: "test-session".to_string(),
            parent_session_id: None,
            title: Some("Updated Title".to_string()),
            model: "gpt-4".to_string(),
            source: "terminal".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: Some("branched".to_string()),
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        };

        adapter
            .update_session("test-session", updated_metadata)
            .unwrap();

        let retrieved = adapter.get_session("test-session").unwrap().unwrap();
        // Note: update_session uses upsert which updates the session_key (title)
        // The title should be updated to "Updated Title"
        assert_eq!(retrieved.title, Some("Updated Title".to_string()));
        assert_eq!(retrieved.end_reason, Some("branched".to_string()));
    }

    #[test]
    fn test_adapter_get_set_title() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let adapter = SessionStoreAdapter::new(store, "principal-adapter-test").unwrap();

        let metadata = SessionMetadata {
            principal_id: "principal-adapter-test".to_string(),
            session_id: "test-session".to_string(),
            parent_session_id: None,
            title: Some("Original".to_string()),
            model: "gpt-4".to_string(),
            source: "terminal".to_string(),
            chat_id: Some("default".to_string()),
            user_id: None,
            thread_id: None,
            end_reason: None,
            created_at: "2026-04-17T00:00:00Z".to_string(),
            ended_at: None,
        };

        adapter.create_session(metadata).unwrap();

        adapter.set_title("test-session", "New Title").unwrap();

        let title = adapter.get_title("test-session").unwrap().unwrap();
        assert_eq!(title, "New Title");
    }

    #[test]
    fn test_adapter_copy_history_requires_event_ledger() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let adapter = SessionStoreAdapter::new(store, "principal-adapter-test").unwrap();

        let err = adapter.copy_history("from", "to").unwrap_err();
        assert!(err.contains("requires EventLedger"), "err={}", err);
    }

    #[test]
    fn test_adapter_copy_history_writes_lineage_events() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let ledger = zaion_ledger::EventLedger::new(temp.path());
        let keypair = zaion_crypto::ZaionKeypair::generate();
        let parent_namespace = zaion_types::session::NamespaceKey("parent-session".to_string());

        let first_source = ledger
            .append_signed_event_with_parent(
                &keypair,
                &parent_namespace,
                "channel.received",
                serde_json::json!({"body": "hello"}),
                None,
                None,
            )
            .unwrap();
        let second_source = ledger
            .append_signed_event_with_parent(
                &keypair,
                &parent_namespace,
                "answer.trace",
                serde_json::json!({"body": "hi"}),
                None,
                Some(&first_source),
            )
            .unwrap();

        let adapter = SessionStoreAdapter::new_with_ledger(store, ledger, keypair).unwrap();
        let copied = adapter
            .copy_history("parent-session", "child-session")
            .unwrap();
        assert_eq!(copied, 2);

        let child_ledger = zaion_ledger::EventLedger::new(temp.path());
        let child_events = child_ledger
            .list_events(
                &zaion_types::session::SessionKey("child-session".to_string()),
                Some("session.history.copied"),
                10,
            )
            .unwrap();
        assert_eq!(child_events.len(), 2);

        let copied_sources = child_events
            .iter()
            .map(|event| {
                assert!(event.signature.is_some());
                assert_eq!(
                    event.payload["schema"],
                    serde_json::json!("zaion.session_history_copy.v1")
                );
                assert_eq!(
                    event.payload["from_session"],
                    serde_json::json!("parent-session")
                );
                assert_eq!(
                    event.payload["to_session"],
                    serde_json::json!("child-session")
                );
                assert_eq!(
                    event.payload["source_event_id"],
                    serde_json::json!(event.parent_event_id.as_ref().unwrap().0.clone())
                );
                assert_eq!(
                    event.payload["source_signature_present"],
                    serde_json::json!(true)
                );
                assert!(event.payload["source_payload"].is_object());
                event.payload["source_event_id"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();

        assert!(copied_sources.contains(&first_source.0));
        assert!(copied_sources.contains(&second_source.0));
    }

    #[test]
    fn test_adapter_rejects_default_principal() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let err = match SessionStoreAdapter::new(store, "default") {
            Ok(_) => panic!("default principal must be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("production-safe principal"));
    }

    #[test]
    fn test_adapter_persists_real_principal() {
        let temp = NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        let adapter = SessionStoreAdapter::new(store, "principal-real").unwrap();

        adapter
            .create_session(SessionMetadata {
                principal_id: "principal-real".to_string(),
                session_id: "test-session".to_string(),
                parent_session_id: None,
                title: Some("Test Session".to_string()),
                model: "gpt-4".to_string(),
                source: "terminal".to_string(),
                chat_id: Some("default".to_string()),
                user_id: None,
                thread_id: None,
                end_reason: None,
                created_at: "2026-04-17T00:00:00Z".to_string(),
                ended_at: None,
            })
            .unwrap();

        let retrieved = adapter.get_session("test-session").unwrap().unwrap();
        assert_eq!(retrieved.principal_id, "principal-real");
    }
}
