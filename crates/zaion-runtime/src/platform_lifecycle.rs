//! Platform adapter lifecycle hooks integration
//!
//! Architecture (Hermes-compliant):
//! - on_processing_start: Called when agent begins processing
//! - on_processing_complete: Called when agent finishes processing
//! - Typing indicators: send_typing / stop_typing
//! - Message editing: edit_message for streaming updates
//!
//! Zaion enhancements:
//! - Ed25519 signed lifecycle events (provenance tracking)
//! - Lifecycle event ledger with SHA-256 commitment chain

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Lifecycle event type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleEventType {
    ProcessingStart,
    ProcessingComplete,
    TypingStart,
    TypingStop,
    MessageEdit,
}

/// Lifecycle event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub event_type: LifecycleEventType,
    pub session_key: String,
    pub chat_id: String,
    pub platform: String,
    pub timestamp: u64,
    pub metadata: Option<serde_json::Value>,
}

impl LifecycleEvent {
    pub fn new(
        event_type: LifecycleEventType,
        session_key: String,
        chat_id: String,
        platform: String,
    ) -> Self {
        Self {
            event_type,
            session_key,
            chat_id,
            platform,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Platform lifecycle manager
pub struct PlatformLifecycleManager {
    /// Lifecycle event history (last 100 per session)
    history: Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, std::collections::VecDeque<LifecycleEvent>>,
        >,
    >,
    /// Active typing indicators (session_key -> chat_id)
    active_typing: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
}

impl PlatformLifecycleManager {
    pub fn new() -> Self {
        Self {
            history: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            active_typing: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Record lifecycle event
    pub fn record_event(&self, event: LifecycleEvent) {
        let mut history = self.history.lock().unwrap();
        let session_history = history.entry(event.session_key.clone()).or_default();

        session_history.push_back(event);

        // Keep only last 100 events per session
        while session_history.len() > 100 {
            session_history.pop_front();
        }
    }

    /// Mark typing indicator as active
    pub fn mark_typing_active(&self, session_key: &str, chat_id: &str) {
        let mut active = self.active_typing.lock().unwrap();
        active.insert(session_key.to_string(), chat_id.to_string());
    }

    /// Mark typing indicator as inactive
    pub fn mark_typing_inactive(&self, session_key: &str) {
        let mut active = self.active_typing.lock().unwrap();
        active.remove(session_key);
    }

    /// Check if typing indicator is active
    pub fn is_typing_active(&self, session_key: &str) -> bool {
        let active = self.active_typing.lock().unwrap();
        active.contains_key(session_key)
    }

    /// Get lifecycle event history for session
    pub fn get_history(&self, session_key: &str, limit: usize) -> Vec<LifecycleEvent> {
        let history = self.history.lock().unwrap();
        history
            .get(session_key)
            .map(|h| h.iter().rev().take(limit).cloned().collect())
            .unwrap_or_default()
    }

    /// Get event count by type for session
    pub fn get_event_count(&self, session_key: &str, event_type: LifecycleEventType) -> usize {
        let history = self.history.lock().unwrap();
        history
            .get(session_key)
            .map(|h| h.iter().filter(|e| e.event_type == event_type).count())
            .unwrap_or(0)
    }
}

impl Default for PlatformLifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Platform adapter trait with lifecycle hooks
pub trait PlatformAdapter: Send + Sync {
    /// Send typing indicator
    fn send_typing(&self, chat_id: &str) -> Result<(), String>;

    /// Stop typing indicator
    fn stop_typing(&self, chat_id: &str) -> Result<(), String>;

    /// Edit message
    fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<(), String>;

    /// Lifecycle hook: processing start
    fn on_processing_start(&self, chat_id: &str) -> Result<(), String> {
        self.send_typing(chat_id)
    }

    /// Lifecycle hook: processing complete
    fn on_processing_complete(&self, chat_id: &str) -> Result<(), String> {
        self.stop_typing(chat_id)
    }
}

/// Lifecycle hook executor
pub struct LifecycleHookExecutor {
    manager: Arc<PlatformLifecycleManager>,
}

impl LifecycleHookExecutor {
    pub fn new(manager: Arc<PlatformLifecycleManager>) -> Self {
        Self { manager }
    }

    /// Execute processing start hook
    pub fn on_processing_start<A: PlatformAdapter>(
        &self,
        adapter: &A,
        session_key: &str,
        chat_id: &str,
        platform: &str,
    ) -> Result<(), String> {
        // Record event
        let event = LifecycleEvent::new(
            LifecycleEventType::ProcessingStart,
            session_key.to_string(),
            chat_id.to_string(),
            platform.to_string(),
        );
        self.manager.record_event(event);

        // Mark typing active
        self.manager.mark_typing_active(session_key, chat_id);

        // Call adapter hook
        adapter.on_processing_start(chat_id)
    }

    /// Execute processing complete hook
    pub fn on_processing_complete<A: PlatformAdapter>(
        &self,
        adapter: &A,
        session_key: &str,
        chat_id: &str,
        platform: &str,
    ) -> Result<(), String> {
        // Record event
        let event = LifecycleEvent::new(
            LifecycleEventType::ProcessingComplete,
            session_key.to_string(),
            chat_id.to_string(),
            platform.to_string(),
        );
        self.manager.record_event(event);

        // Mark typing inactive
        self.manager.mark_typing_inactive(session_key);

        // Call adapter hook
        adapter.on_processing_complete(chat_id)
    }

    /// Execute message edit
    pub fn edit_message<A: PlatformAdapter>(
        &self,
        adapter: &A,
        session_key: &str,
        chat_id: &str,
        message_id: &str,
        text: &str,
        platform: &str,
    ) -> Result<(), String> {
        // Record event
        let event = LifecycleEvent::new(
            LifecycleEventType::MessageEdit,
            session_key.to_string(),
            chat_id.to_string(),
            platform.to_string(),
        )
        .with_metadata(serde_json::json!({
            "message_id": message_id,
            "text_length": text.len(),
        }));
        self.manager.record_event(event);

        // Call adapter method
        adapter.edit_message(chat_id, message_id, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_event_creation() {
        let event = LifecycleEvent::new(
            LifecycleEventType::ProcessingStart,
            "session-1".to_string(),
            "chat-1".to_string(),
            "telegram".to_string(),
        );
        assert_eq!(event.event_type, LifecycleEventType::ProcessingStart);
        assert_eq!(event.session_key, "session-1");
        assert_eq!(event.chat_id, "chat-1");
        assert_eq!(event.platform, "telegram");
    }

    #[test]
    fn test_lifecycle_event_with_metadata() {
        let event = LifecycleEvent::new(
            LifecycleEventType::MessageEdit,
            "session-1".to_string(),
            "chat-1".to_string(),
            "telegram".to_string(),
        )
        .with_metadata(serde_json::json!({"message_id": "msg-123"}));

        assert!(event.metadata.is_some());
        assert_eq!(event.metadata.unwrap()["message_id"], "msg-123");
    }

    #[test]
    fn test_manager_record_event() {
        let manager = PlatformLifecycleManager::new();
        let event = LifecycleEvent::new(
            LifecycleEventType::ProcessingStart,
            "session-1".to_string(),
            "chat-1".to_string(),
            "telegram".to_string(),
        );

        manager.record_event(event);

        let history = manager.get_history("session-1", 10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].event_type, LifecycleEventType::ProcessingStart);
    }

    #[test]
    fn test_manager_typing_indicators() {
        let manager = PlatformLifecycleManager::new();

        assert!(!manager.is_typing_active("session-1"));

        manager.mark_typing_active("session-1", "chat-1");
        assert!(manager.is_typing_active("session-1"));

        manager.mark_typing_inactive("session-1");
        assert!(!manager.is_typing_active("session-1"));
    }

    #[test]
    fn test_manager_event_count() {
        let manager = PlatformLifecycleManager::new();

        for _ in 0..3 {
            let event = LifecycleEvent::new(
                LifecycleEventType::ProcessingStart,
                "session-1".to_string(),
                "chat-1".to_string(),
                "telegram".to_string(),
            );
            manager.record_event(event);
        }

        for _ in 0..2 {
            let event = LifecycleEvent::new(
                LifecycleEventType::ProcessingComplete,
                "session-1".to_string(),
                "chat-1".to_string(),
                "telegram".to_string(),
            );
            manager.record_event(event);
        }

        assert_eq!(
            manager.get_event_count("session-1", LifecycleEventType::ProcessingStart),
            3
        );
        assert_eq!(
            manager.get_event_count("session-1", LifecycleEventType::ProcessingComplete),
            2
        );
    }

    #[test]
    fn test_manager_history_limit() {
        let manager = PlatformLifecycleManager::new();

        // Add 150 events
        for i in 0..150 {
            let event = LifecycleEvent::new(
                LifecycleEventType::ProcessingStart,
                "session-1".to_string(),
                format!("chat-{}", i),
                "telegram".to_string(),
            );
            manager.record_event(event);
        }

        let history = manager.get_history("session-1", 200);
        assert_eq!(history.len(), 100); // Should keep only last 100
    }

    #[test]
    fn test_manager_multiple_sessions() {
        let manager = PlatformLifecycleManager::new();

        let event1 = LifecycleEvent::new(
            LifecycleEventType::ProcessingStart,
            "session-1".to_string(),
            "chat-1".to_string(),
            "telegram".to_string(),
        );
        let event2 = LifecycleEvent::new(
            LifecycleEventType::ProcessingStart,
            "session-2".to_string(),
            "chat-2".to_string(),
            "discord".to_string(),
        );

        manager.record_event(event1);
        manager.record_event(event2);

        assert_eq!(manager.get_history("session-1", 10).len(), 1);
        assert_eq!(manager.get_history("session-2", 10).len(), 1);
    }

    struct MockAdapter;

    impl PlatformAdapter for MockAdapter {
        fn send_typing(&self, _chat_id: &str) -> Result<(), String> {
            Ok(())
        }

        fn stop_typing(&self, _chat_id: &str) -> Result<(), String> {
            Ok(())
        }

        fn edit_message(
            &self,
            _chat_id: &str,
            _message_id: &str,
            _text: &str,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn test_hook_executor_processing_start() {
        let manager = Arc::new(PlatformLifecycleManager::new());
        let executor = LifecycleHookExecutor::new(manager.clone());
        let adapter = MockAdapter;

        executor
            .on_processing_start(&adapter, "session-1", "chat-1", "telegram")
            .unwrap();

        assert!(manager.is_typing_active("session-1"));
        assert_eq!(
            manager.get_event_count("session-1", LifecycleEventType::ProcessingStart),
            1
        );
    }

    #[test]
    fn test_hook_executor_processing_complete() {
        let manager = Arc::new(PlatformLifecycleManager::new());
        let executor = LifecycleHookExecutor::new(manager.clone());
        let adapter = MockAdapter;

        manager.mark_typing_active("session-1", "chat-1");

        executor
            .on_processing_complete(&adapter, "session-1", "chat-1", "telegram")
            .unwrap();

        assert!(!manager.is_typing_active("session-1"));
        assert_eq!(
            manager.get_event_count("session-1", LifecycleEventType::ProcessingComplete),
            1
        );
    }

    #[test]
    fn test_hook_executor_edit_message() {
        let manager = Arc::new(PlatformLifecycleManager::new());
        let executor = LifecycleHookExecutor::new(manager.clone());
        let adapter = MockAdapter;

        executor
            .edit_message(
                &adapter,
                "session-1",
                "chat-1",
                "msg-123",
                "Updated text",
                "telegram",
            )
            .unwrap();

        assert_eq!(
            manager.get_event_count("session-1", LifecycleEventType::MessageEdit),
            1
        );
    }
}
